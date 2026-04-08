//! Input parser for `__ankyra_assemble!`.
//!
//! The assembler is invoked by `ankyra_config!` (Task 11) with a token
//! stream of the shape
//!
//! ```text
//! config = {
//!     transport_path = <path>,
//!     transport_ty   = <ty>,
//!     context_ty     = <ty>,
//!     static_strings = [ "s1", "s2", ... ]
//! },
//! items = [ <tuple>, <tuple>, ... ]
//! ```
//!
//! Each `<tuple>` is a parenthesized carrier group emitted by one of the
//! `#[klipper_*]` attribute macros. The first element is a bare ident
//! identifying the kind (`command`, `reply`, `output`, `constant`,
//! `enumeration`); the remaining three elements are `"name"`, `"format"`,
//! and a `syn::Path` to the descriptor / dispatch helper.
//!
//! Commands / replies / outputs flow into [`ParsedInput::items`] so the
//! Task 10 sort stage can sort them against the reserved `identify` /
//! `identify_response` / `shutdown` items. Constants and enumerations go
//! into [`ParsedInput::definitions`] — Task 12 will consume that bucket
//! when it synthesizes the data dictionary; Task 10 ignores it.
//!
//! The parser is deliberately lenient about missing `config` sub-fields:
//! `static_strings = [ ... ]` may be omitted entirely and defaults to an
//! empty `Vec<String>`. `transport_path`, `transport_ty`, and `context_ty`
//! are also optional at Task 10 — Task 11 will require them when it
//! switches `__ankyra_assemble!` from emitting a `()` placeholder to
//! emitting a real transport binding.

#![allow(dead_code)]

use proc_macro2::TokenStream as TokenStream2;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Ident, LitStr, Path, Token, braced, bracketed, parenthesized};

use crate::sort::{ItemInput, ItemKind};

/// A `constant`- or `enumeration`-kind carrier tuple as delivered by the
/// macro crates. Task 10 does not consume this; Task 12 will read it when
/// it emits the data dictionary.
#[derive(Debug, Clone)]
pub(crate) struct DefinitionInput {
    pub kind: DefinitionKind,
    pub name: String,
    pub value_or_format: String,
    pub descriptor_path: TokenStream2,
}

/// Kind discriminant for definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefinitionKind {
    Constant,
    Enumeration,
}

/// Full parse result for an `__ankyra_assemble!` invocation.
///
/// Fields are `pub(crate)` because the only consumer is the crate-root
/// proc-macro entry in `lib.rs`; exposing them outside the crate would
/// require stabilizing field names that Task 11/12 may still rename.
#[derive(Debug, Default)]
pub(crate) struct ParsedInput {
    pub items: Vec<ItemInput>,
    pub static_strings: Vec<String>,
    pub transport_path: Option<TokenStream2>,
    pub transport_ty: Option<TokenStream2>,
    pub context_ty: Option<TokenStream2>,
    pub definitions: Vec<DefinitionInput>,
}

/// Parse the token stream handed to `__ankyra_assemble!`.
///
/// Errors are reported as `syn::Error` so the proc-macro entry can convert
/// them to a compile-time diagnostic via `Error::to_compile_error`. This
/// keeps parser-level failures span-accurate without needing
/// `proc_macro_error2::abort!`, which is reserved for semantic failures
/// (name collisions, reserved-name shadowing, etc.) coming out of
/// `sort::assemble`.
pub(crate) fn parse(tokens: TokenStream2) -> Result<ParsedInput, syn::Error> {
    syn::parse2::<ParsedInput>(tokens)
}

/// syn parse entry that handles the top-level `config = { ... }, items = [ ... ]`
/// shape. Order of the two keys is fixed for determinism; a future revision
/// may accept either ordering if there is a reason.
impl Parse for ParsedInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut out = ParsedInput::default();

        // `config = { ... }`
        let config_key: Ident = input.parse()?;
        if config_key != "config" {
            return Err(syn::Error::new(
                config_key.span(),
                "expected `config = { .. }`",
            ));
        }
        let _: Token![=] = input.parse()?;
        let config_body;
        braced!(config_body in input);
        parse_config(&config_body, &mut out)?;

        let _: Token![,] = input.parse()?;

        // `items = [ ... ]`
        let items_key: Ident = input.parse()?;
        if items_key != "items" {
            return Err(syn::Error::new(
                items_key.span(),
                "expected `items = [ .. ]`",
            ));
        }
        let _: Token![=] = input.parse()?;
        let items_body;
        bracketed!(items_body in input);
        parse_items(&items_body, &mut out)?;

        // Trailing commas are tolerated but not required.
        let _ = input.parse::<Token![,]>();
        Ok(out)
    }
}

/// Parse the interior of `config = { ... }`.
///
/// All sub-keys are optional; unknown keys produce a span-pointed error so
/// typos don't silently drop configuration on the floor.
fn parse_config(input: ParseStream<'_>, out: &mut ParsedInput) -> syn::Result<()> {
    while !input.is_empty() {
        let key: Ident = input.parse()?;
        let _: Token![=] = input.parse()?;
        match key.to_string().as_str() {
            "transport_path" => {
                let path: Path = input.parse()?;
                out.transport_path = Some(path_to_tokens(&path));
            }
            "transport_ty" => {
                let ty: syn::Type = input.parse()?;
                out.transport_ty = Some(type_to_tokens(&ty));
            }
            "context_ty" => {
                let ty: syn::Type = input.parse()?;
                out.context_ty = Some(type_to_tokens(&ty));
            }
            "static_strings" => {
                let list;
                bracketed!(list in input);
                let strings: Punctuated<LitStr, Token![,]> =
                    Punctuated::<LitStr, Token![,]>::parse_terminated(&list)?;
                out.static_strings = strings.into_iter().map(|s| s.value()).collect();
            }
            other => {
                return Err(syn::Error::new(
                    key.span(),
                    format!("unknown config key `{other}`"),
                ));
            }
        }
        // Accept optional trailing comma between keys.
        let _ = input.parse::<Token![,]>();
    }
    Ok(())
}

/// Parse the interior of `items = [ ... ]`.
///
/// Each item is either
///
/// 1. A parenthesized carrier tuple `(kind, "name", "format", path)` — the
///    shape the `#[klipper_*]` carrier macros expand to. Used by
///    synthetic/inline call sites and the parser's unit tests.
///
/// 2. A bare carrier macro invocation `<path>::__ankyra_item_<kind>_<name>!()`.
///    The CPS-fold accumulator produced by `ankyra_config!` threads
///    unexpanded carrier calls through to `__ankyra_assemble!`: proc-macros
///    do not trigger expansion of declarative macros within their argument
///    stream, so a literal `foo!()` on the way in stays `foo!()` here. We
///    therefore recognize the macro-call shape and extract the item's kind
///    and protocol name from the trailing `__ankyra_item_<kind>_<name>`
///    path segment. The carrier's full `message_format`, `descriptor_path`,
///    and `dispatch_path` are not recovered at this stage — Task 12 will
///    grow this parser to emit a rendezvous const whose initializer invokes
///    the carrier at a position where rustc expands it, replacing the
///    placeholder metadata with the carrier's tuple contents.
fn parse_items(input: ParseStream<'_>, out: &mut ParsedInput) -> syn::Result<()> {
    while !input.is_empty() {
        if input.peek(syn::token::Paren) {
            parse_inline_tuple(input, out)?;
        } else {
            parse_carrier_call(input, out)?;
        }
        // Optional comma between items.
        let _ = input.parse::<Token![,]>();
    }
    Ok(())
}

/// Parse one inline carrier tuple `(kind, "name", "format", path)`.
fn parse_inline_tuple(input: ParseStream<'_>, out: &mut ParsedInput) -> syn::Result<()> {
    let tuple;
    parenthesized!(tuple in input);
    let kind_ident: Ident = tuple.parse()?;
    let _: Token![,] = tuple.parse()?;
    let name: LitStr = tuple.parse()?;
    let _: Token![,] = tuple.parse()?;
    let format: LitStr = tuple.parse()?;
    let _: Token![,] = tuple.parse()?;
    let path: Path = tuple.parse()?;
    // Tolerate a trailing comma inside the tuple.
    let _ = tuple.parse::<Token![,]>();

    let path_tokens = path_to_tokens(&path);
    route_item(
        &kind_ident,
        name.value(),
        Some(format.value()),
        Some(path_tokens),
        out,
    )
}

/// Parse one unexpanded carrier macro call `<path>::__ankyra_item_<kind>_<name>!()`
/// from the fold accumulator.
fn parse_carrier_call(input: ParseStream<'_>, out: &mut ParsedInput) -> syn::Result<()> {
    let path: Path = input.parse()?;
    let _: Token![!] = input.parse()?;
    // Consume the `()` argument list. It is always empty for carrier macros,
    // but `parenthesized!` still needs the group to be present so the cursor
    // advances past it.
    let args;
    parenthesized!(args in input);
    // Drain any content the carrier macro might carry (current carriers are
    // nullary, but tolerating forward-compatible extensions is cheap).
    let _ = args.parse::<TokenStream2>()?;

    let last = path
        .segments
        .last()
        .ok_or_else(|| syn::Error::new_spanned(&path, "carrier macro path has no final segment"))?;
    let ident_str = last.ident.to_string();

    // Strip the `__ankyra_item_` prefix, then split `<kind>_<name>`.
    let rest = ident_str.strip_prefix("__ankyra_item_").ok_or_else(|| {
        syn::Error::new(
            last.ident.span(),
            format!(
                "unexpected item token `{ident_str}`; expected either a \
                 parenthesized carrier tuple or a `<path>::__ankyra_item_<kind>_<name>!()` \
                 macro invocation"
            ),
        )
    })?;
    let (kind_str, name) = split_kind_and_name(rest).ok_or_else(|| {
        syn::Error::new(
            last.ident.span(),
            format!(
                "carrier macro ident `{ident_str}` must match \
                 `__ankyra_item_<kind>_<name>` with `<kind>` one of \
                 command, reply, output, constant, enumeration"
            ),
        )
    })?;

    let kind_ident = Ident::new(&kind_str, last.ident.span());
    // The carrier's format string is not available here because the carrier
    // macro did not expand. Route the item with `message_format = None` so
    // the sort stage knows it is a placeholder; Task 12 will replace this
    // with the real format once the carrier rendezvous const lands.
    route_item(&kind_ident, name, None, None, out)
}

/// Split a `<kind>_<name>` tail string into `(kind, name)` if `<kind>` is a
/// known carrier kind. Returns `None` for unknown kinds.
fn split_kind_and_name(tail: &str) -> Option<(String, String)> {
    for kind in ["command", "reply", "output", "constant", "enumeration"] {
        if let Some(rest) = tail.strip_prefix(kind)
            && let Some(name) = rest.strip_prefix('_')
            && !name.is_empty()
        {
            return Some((kind.to_string(), name.to_string()));
        }
    }
    None
}

/// Route one item into [`ParsedInput::items`] or [`ParsedInput::definitions`]
/// based on its kind ident. Shared between the inline-tuple and
/// carrier-call parse paths so dispatch logic stays single-sourced.
fn route_item(
    kind_ident: &Ident,
    name: String,
    message_format: Option<String>,
    path_tokens: Option<TokenStream2>,
    out: &mut ParsedInput,
) -> syn::Result<()> {
    match kind_ident.to_string().as_str() {
        "command" => out.items.push(ItemInput {
            kind: ItemKind::Command,
            name,
            message_format,
            descriptor_path: None,
            dispatch_path: path_tokens,
        }),
        "reply" => out.items.push(ItemInput {
            kind: ItemKind::Reply,
            name,
            message_format,
            descriptor_path: path_tokens,
            dispatch_path: None,
        }),
        "output" => out.items.push(ItemInput {
            kind: ItemKind::Output,
            name,
            message_format,
            descriptor_path: path_tokens,
            dispatch_path: None,
        }),
        "constant" => out.definitions.push(DefinitionInput {
            kind: DefinitionKind::Constant,
            name,
            value_or_format: message_format.unwrap_or_default(),
            descriptor_path: path_tokens.unwrap_or_default(),
        }),
        "enumeration" => out.definitions.push(DefinitionInput {
            kind: DefinitionKind::Enumeration,
            name,
            value_or_format: message_format.unwrap_or_default(),
            descriptor_path: path_tokens.unwrap_or_default(),
        }),
        other => {
            return Err(syn::Error::new(
                kind_ident.span(),
                format!(
                    "unknown carrier tuple kind `{other}`; expected one of \
                     `command`, `reply`, `output`, `constant`, `enumeration`"
                ),
            ));
        }
    }
    Ok(())
}

/// Convert a parsed `syn::Path` to an owned `TokenStream2`. Wrapping this
/// once keeps the call sites above tidy and makes the intent ("store this
/// for later re-emission") explicit.
fn path_to_tokens(path: &Path) -> TokenStream2 {
    quote::ToTokens::to_token_stream(path)
}

/// Convert a parsed `syn::Type` to an owned `TokenStream2`. See
/// `path_to_tokens`.
fn type_to_tokens(ty: &syn::Type) -> TokenStream2 {
    quote::ToTokens::to_token_stream(ty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    #[test]
    fn parses_empty_items_and_empty_config() {
        let input = quote! {
            config = {},
            items = []
        };
        let parsed = parse(input).expect("empty config + empty items must parse");
        assert!(parsed.items.is_empty());
        assert!(parsed.static_strings.is_empty());
        assert!(parsed.definitions.is_empty());
        assert!(parsed.transport_path.is_none());
        assert!(parsed.transport_ty.is_none());
        assert!(parsed.context_ty.is_none());
    }

    #[test]
    fn parses_single_command_tuple() {
        let input = quote! {
            config = { static_strings = ["alpha"] },
            items = [
                (command, "ping", "ping", crate::__ankyra_dispatch_ping)
            ]
        };
        let parsed = parse(input).expect("single-command input must parse");
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].kind, ItemKind::Command);
        assert_eq!(parsed.items[0].name, "ping");
        assert_eq!(
            parsed.items[0].message_format.as_deref(),
            Some("ping"),
            "message format should round-trip from the carrier tuple"
        );
        assert!(parsed.items[0].dispatch_path.is_some());
        assert_eq!(parsed.static_strings, vec!["alpha".to_string()]);
    }

    #[test]
    fn routes_constant_and_enumeration_into_definitions() {
        let input = quote! {
            config = {},
            items = [
                (constant, "MCU_FREQ", "168000000u32", crate::__ankyra_descriptor_MCU_FREQ),
                (enumeration, "pin_name", "encoded", crate::__ankyra_descriptor_pin_name)
            ]
        };
        let parsed = parse(input).expect("definitions input must parse");
        assert!(
            parsed.items.is_empty(),
            "definitions must not land in items"
        );
        assert_eq!(parsed.definitions.len(), 2);
        assert_eq!(parsed.definitions[0].kind, DefinitionKind::Constant);
        assert_eq!(parsed.definitions[0].name, "MCU_FREQ");
        assert_eq!(parsed.definitions[1].kind, DefinitionKind::Enumeration);
        assert_eq!(parsed.definitions[1].name, "pin_name");
    }

    #[test]
    fn rejects_unknown_tuple_kind() {
        let input = quote! {
            config = {},
            items = [ (wat, "x", "x", crate::x) ]
        };
        let err = parse(input).expect_err("unknown kind must be rejected");
        assert!(err.to_string().contains("unknown carrier tuple kind"));
    }

    #[test]
    fn parses_unexpanded_carrier_macro_call() {
        // When `ankyra_config!`'s CPS fold feeds us the accumulator, the
        // carrier macros are still unexpanded — proc-macro input is not
        // pre-expanded by rustc. The parser must recognise this shape and
        // recover the kind + name from the macro ident. By the time the
        // accumulator lands here `$crate` has already been resolved to a
        // concrete crate path by rustc (e.g. `::ankyra_macros`), so this
        // test mirrors that post-resolution shape.
        let input = quote! {
            config = {},
            items = [
                ::ankyra_macros::__ankyra_item_command_emergency_stop!(),
                ::ankyra_macros::__ankyra_item_reply_PingReply!(),
            ]
        };
        let parsed = parse(input).expect("carrier-call input must parse");
        assert_eq!(parsed.items.len(), 2);
        assert_eq!(parsed.items[0].kind, ItemKind::Command);
        assert_eq!(parsed.items[0].name, "emergency_stop");
        assert!(
            parsed.items[0].message_format.is_none(),
            "message_format is a placeholder until Task 12's rendezvous \
             const resolves the carrier"
        );
        assert_eq!(parsed.items[1].kind, ItemKind::Reply);
        assert_eq!(parsed.items[1].name, "PingReply");
    }

    #[test]
    fn rejects_malformed_carrier_ident() {
        // Arbitrary macro calls without the `__ankyra_item_<kind>_<name>`
        // shape must error rather than silently landing in the parser.
        let input = quote! {
            config = {},
            items = [ some::random::macro_call!() ]
        };
        let err = parse(input).expect_err("malformed ident must be rejected");
        assert!(
            err.to_string()
                .contains("expected either a parenthesized carrier tuple"),
            "wrong diagnostic: {err}"
        );
    }
}
