//! Input parser for `__ankyra_assemble!`.
//!
//! The assembler is invoked by `ankyra_config!` with a token
//! stream of the shape
//!
//! ```text
//! config = {
//!     transport_path = <path>,
//!     transport_ty   = <ty>,
//!     context_ty     = <ty>,
//!     static_strings = [ "s1", "s2", ... ],
//!     app            = <expr>,   // optional
//!     version        = <expr>,   // optional
//!     build_versions = <expr>,   // optional
//!     license        = <expr>,   // optional
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
//! sort stage can sort them against the reserved `identify` /
//! `identify_response` / `shutdown` items. Constants and enumerations go
//! into [`ParsedInput::definitions`], which only the dictionary builder
//! consumes.
//!
//! The parser is deliberately lenient about missing `config` sub-fields:
//! `static_strings = [ ... ]` may be omitted entirely and defaults to an
//! empty `Vec<String>`. `transport_path`, `transport_ty`, and `context_ty`
//! are also optional here; the `__ankyra_assemble!` entry point aborts
//! when any of them is missing.

use proc_macro2::TokenStream as TokenStream2;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Ident, LitStr, Path, Token, braced, bracketed, parenthesized};

use crate::sort::{ItemInput, ItemKind};

/// A `constant`- or `enumeration`-kind carrier tuple as delivered by the
/// macro crates. Only the dictionary builder reads it.
#[derive(Debug, Clone)]
pub(crate) struct DefinitionInput {
    pub kind: DefinitionKind,
    pub name: String,
    pub value_or_format: String,
    /// Path to the `__ankyra_item_<kind>_<name>!` carrier macro. `None` only
    /// for inline-tuple fixtures; the dictionary builder rejects a
    /// carrier-backed definition that arrives without a `sibling_scope`.
    pub carrier_path: Option<TokenStream2>,
    /// Module scope where this definition's sibling
    /// `__ANKYRA_VALUE_<kind>_<name>` / `__ANKYRA_NAME_<kind>_<name>` consts
    /// live: `Some($crate)` for crate-root items, `Some($crate::submod)` for
    /// submodule items. `None` only for inline-tuple test fixtures.
    pub sibling_scope: Option<TokenStream2>,
}

impl DefinitionInput {
    pub(crate) fn kind_tag(&self) -> &'static str {
        self.kind.tag()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefinitionKind {
    Constant,
    Enumeration,
}

impl DefinitionKind {
    /// Carrier tuple kind ident, as used in sibling-const idents like
    /// `__ANKYRA_VALUE_constant_FOO`.
    pub(crate) fn tag(self) -> &'static str {
        match self {
            Self::Constant => "constant",
            Self::Enumeration => "enumeration",
        }
    }
}

/// Full parse result for an `__ankyra_assemble!` invocation.
#[derive(Debug, Default)]
pub(crate) struct ParsedInput {
    pub items: Vec<ItemInput>,
    pub static_strings: Vec<String>,
    pub transport_path: Option<TokenStream2>,
    pub transport_ty: Option<TokenStream2>,
    pub context_ty: Option<TokenStream2>,
    pub definitions: Vec<DefinitionInput>,
    /// User override for the dictionary's `"app"` trailer field.
    /// `None` triggers the default (`"ankyra"`) at dictionary-emit time.
    pub app: Option<TokenStream2>,
    /// User override for the dictionary's `"version"` trailer field.
    /// `None` triggers the default (`"ankyra-v0.1"`).
    pub version: Option<TokenStream2>,
    /// User override for the dictionary's `"build_versions"` trailer
    /// field. `None` triggers ankyra-assemble's own default —
    /// `concat!("ankyra-", env!("CARGO_PKG_VERSION"))` resolved at the
    /// assembler's compile site so the value is ankyra's own package
    /// version, never the consuming crate's.
    pub build_versions: Option<TokenStream2>,
    /// User override for the dictionary's `"license"` trailer field.
    /// `None` triggers the default (`"MIT OR Apache-2.0"`).
    pub license: Option<TokenStream2>,
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

/// Parses the top-level `config = { ... }, items = [ ... ]` shape. The key
/// order is fixed.
impl Parse for ParsedInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut out = ParsedInput::default();

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
            "app" => {
                let expr: syn::Expr = input.parse()?;
                out.app = Some(quote::ToTokens::to_token_stream(&expr));
            }
            "version" => {
                let expr: syn::Expr = input.parse()?;
                out.version = Some(quote::ToTokens::to_token_stream(&expr));
            }
            "build_versions" => {
                let expr: syn::Expr = input.parse()?;
                out.build_versions = Some(quote::ToTokens::to_token_stream(&expr));
            }
            "license" => {
                let expr: syn::Expr = input.parse()?;
                out.license = Some(quote::ToTokens::to_token_stream(&expr));
            }
            other => {
                return Err(syn::Error::new(
                    key.span(),
                    format!("unknown config key `{other}`"),
                ));
            }
        }
        let _ = input.parse::<Token![,]>();
    }
    Ok(())
}

/// Parse the interior of `items = [ ... ]`.
///
/// Each item is one of
///
/// 1. A parenthesized carrier tuple `(kind, "name", "format", path)` — the
///    shape the `#[klipper_*]` carrier macros expand to. Used by
///    synthetic/inline call sites and the parser's unit tests.
///
/// 2. A wrapped carrier call `{ prefix: (..), <path>!() }`, the form
///    `ankyra_provider!` emits. See [`parse_wrapped_carrier_call`].
///
/// 3. A bare carrier macro invocation `<path>::__ankyra_item_<kind>_<name>!()`.
///    Proc-macros do not expand declarative macros in their argument
///    stream, so the CPS-fold accumulator delivers carrier calls
///    unexpanded. The item's kind and protocol name are recovered from the
///    trailing `__ankyra_item_<kind>_<name>` path segment. The carrier's
///    `message_format` is not recoverable here; the dictionary builder
///    reads the item's sibling `__ANKYRA_FORMAT_<kind>_<name>` const
///    instead.
fn parse_items(input: ParseStream<'_>, out: &mut ParsedInput) -> syn::Result<()> {
    while !input.is_empty() {
        if input.peek(syn::token::Paren) {
            parse_inline_tuple(input, out)?;
        } else if input.peek(syn::token::Brace) {
            parse_wrapped_carrier_call(input, out)?;
        } else {
            parse_carrier_call(input, out)?;
        }
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
    let _ = tuple.parse::<Token![,]>();

    let path_tokens = Some(path_to_tokens(&path));
    let (descriptor_path, dispatch_path) = if kind_ident == "command" {
        (None, path_tokens)
    } else {
        (path_tokens, None)
    };
    let fields = RoutedFields {
        name: name.value(),
        message_format: Some(format.value()),
        descriptor_path,
        dispatch_path,
        ..RoutedFields::default()
    };
    route_item(&kind_ident, fields, out)
}

/// Parse one unexpanded carrier macro call `<path>::__ankyra_item_<kind>_<name>!()`
/// from the fold accumulator.
///
/// The carrier macro path serves double duty: its last segment encodes the
/// item's kind and name, and its prefix points back at the defining crate's
/// root (because every carrier macro is `#[macro_export]`). The parser uses the
/// prefix to reconstruct three sibling paths at the same scope:
///
/// * `<prefix>::__ankyra_dispatch_<name>` — command dispatch fn (only for
///   `command`-kind carriers).
/// * `<prefix>::__ankyra_descriptor_<name>` — reply/output descriptor fn
///   (only for `reply`/`output` carriers; emitted by `#[klipper_reply]`
///   alongside the struct).
/// * `<prefix>::<name>` — the struct type (for `reply`/`output` carriers).
///
/// This works because every `#[klipper_*]` item-level macro emits its
/// descriptor fn, dispatch fn, and carrier macro under the same module —
/// and the `#[macro_export]` carrier is hoisted to the defining crate's
/// root. Therefore when we observe a carrier at `::other_crate::foo::__ankyra_item_reply_PingReply`,
/// the struct `PingReply` and descriptor fn `__ankyra_descriptor_PingReply`
/// live at `::other_crate::foo::`. For same-crate carriers written bare
/// (because the companion macro rewrites `$crate::…` to the bare ident),
/// the prefix is empty and the parser synthesizes a `crate::` qualifier.
fn parse_carrier_call(input: ParseStream<'_>, out: &mut ParsedInput) -> syn::Result<()> {
    let path: Path = input.parse()?;
    let _: Token![!] = input.parse()?;
    let args;
    parenthesized!(args in input);
    let _ = args.parse::<TokenStream2>()?;

    let last = path
        .segments
        .last()
        .ok_or_else(|| syn::Error::new_spanned(&path, "carrier macro path has no final segment"))?;
    let ident_str = last.ident.to_string();

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
    let (kind_str, raw_name) = split_kind_and_name(rest).ok_or_else(|| {
        syn::Error::new(
            last.ident.span(),
            format!(
                "carrier macro ident `{ident_str}` must match \
                 `__ankyra_item_<kind>_<name>` with `<kind>` one of \
                 command, reply, output, constant, enumeration"
            ),
        )
    })?;
    let (name, lifetime_count) = match kind_str.as_str() {
        "reply" | "output" => split_name_with_lifetime_count(&raw_name),
        _ => (raw_name, 0usize),
    };

    let kind_ident = Ident::new(&kind_str, last.ident.span());

    let prefix = Path {
        leading_colon: path.leading_colon,
        segments: path
            .segments
            .iter()
            .take(path.segments.len() - 1)
            .cloned()
            .collect(),
    };
    let prefix_tokens = if prefix.segments.is_empty() && prefix.leading_colon.is_none() {
        None
    } else {
        Some(quote::ToTokens::to_token_stream(&prefix))
    };

    let span = last.ident.span();
    let (descriptor_path, dispatch_path) = match kind_str.as_str() {
        "command" => {
            let dispatch_ident = Ident::new(&format!("__ankyra_dispatch_{name}"), span);
            (
                None,
                Some(join_path(prefix_tokens.as_ref(), &dispatch_ident)),
            )
        }
        "reply" | "output" => {
            let desc_ident = Ident::new(&format!("__ankyra_descriptor_{name}"), span);
            (Some(join_path(prefix_tokens.as_ref(), &desc_ident)), None)
        }
        _ => (None, None),
    };

    let carrier_tokens = Some(quote::ToTokens::to_token_stream(&path));
    let sibling_scope = prefix_tokens.unwrap_or_else(|| quote::quote!($crate));
    let fields = RoutedFields {
        name,
        lifetime_count,
        message_format: None,
        descriptor_path,
        dispatch_path,
        carrier_path: carrier_tokens,
        sibling_scope: Some(sibling_scope),
    };
    route_item(&kind_ident, fields, out)
}

/// Parse one wrapped carrier invocation:
///
/// ```text
/// { prefix: (<prefix_tokens>), <path>::__ankyra_item_<kind>_<name>!() }
/// ```
///
/// The `<prefix_tokens>` are empty (`()`) for crate-root items or a
/// `$crate::…` path for submodule items — see
/// `ankyra-macros/src/provider.rs::carrier_call` for emission.
fn parse_wrapped_carrier_call(input: ParseStream<'_>, out: &mut ParsedInput) -> syn::Result<()> {
    let body;
    braced!(body in input);

    let label: Ident = body.parse()?;
    if label != "prefix" {
        return Err(syn::Error::new(
            label.span(),
            format!("expected `prefix:` in wrapped carrier tuple, got `{label}`"),
        ));
    }
    let _colon: Token![:] = body.parse()?;

    let prefix_body;
    parenthesized!(prefix_body in body);
    let prefix_ts: TokenStream2 = prefix_body.parse()?;
    let sibling_scope = if prefix_ts.is_empty() {
        quote::quote!($crate)
    } else {
        prefix_ts
    };

    let _comma: Token![,] = body.parse()?;

    parse_carrier_call_with_prefix(&body, &sibling_scope, out)?;

    let _ = body.parse::<Token![,]>();
    Ok(())
}

/// Parse the carrier call inside a wrapped entry, using `sibling_scope`
/// verbatim as the sibling-path prefix. The carrier's own path cannot
/// supply it: `#[macro_export]` hoists every carrier to the crate root.
///
/// The carrier path is collected as raw tokens up to the `!` rather than
/// parsed as a `syn::Path`, which rejects the `$crate::…` form that
/// appears verbatim in token streams built from strings (unit tests).
fn parse_carrier_call_with_prefix(
    input: ParseStream<'_>,
    sibling_scope: &TokenStream2,
    out: &mut ParsedInput,
) -> syn::Result<()> {
    let mut path_tts: Vec<proc_macro2::TokenTree> = Vec::new();
    let mut span = input.span();
    loop {
        if input.peek(Token![!]) {
            break;
        }
        if input.is_empty() {
            return Err(syn::Error::new(
                span,
                "expected `!` after carrier macro path",
            ));
        }
        let tt: proc_macro2::TokenTree = input.parse()?;
        span = tt.span();
        path_tts.push(tt);
    }
    let _: Token![!] = input.parse()?;
    let args;
    parenthesized!(args in input);
    let _ = args.parse::<TokenStream2>()?;

    let last_ident = path_tts
        .iter()
        .rev()
        .find_map(|tt| {
            if let proc_macro2::TokenTree::Ident(id) = tt {
                Some(id.clone())
            } else {
                None
            }
        })
        .ok_or_else(|| syn::Error::new(span, "carrier macro path has no final identifier"))?;

    let ident_str = last_ident.to_string();
    let rest = ident_str.strip_prefix("__ankyra_item_").ok_or_else(|| {
        syn::Error::new(
            last_ident.span(),
            format!(
                "unexpected item token `{ident_str}`; expected a \
                 `<path>::__ankyra_item_<kind>_<name>!()` macro invocation"
            ),
        )
    })?;
    let (kind_str, raw_name) = split_kind_and_name(rest).ok_or_else(|| {
        syn::Error::new(
            last_ident.span(),
            format!(
                "carrier macro ident `{ident_str}` must match \
                 `__ankyra_item_<kind>_<name>`"
            ),
        )
    })?;
    let (name, lifetime_count) = match kind_str.as_str() {
        "reply" | "output" => split_name_with_lifetime_count(&raw_name),
        _ => (raw_name, 0usize),
    };

    let kind_ident = Ident::new(&kind_str, last_ident.span());
    let carrier_tokens: TokenStream2 = path_tts.into_iter().collect();

    let (descriptor_path, dispatch_path) = match kind_str.as_str() {
        "command" => {
            let dispatch_ident =
                Ident::new(&format!("__ankyra_dispatch_{name}"), last_ident.span());
            (None, Some(join_path(Some(sibling_scope), &dispatch_ident)))
        }
        "reply" | "output" => {
            let desc_ident = Ident::new(&format!("__ankyra_descriptor_{name}"), last_ident.span());
            (Some(join_path(Some(sibling_scope), &desc_ident)), None)
        }
        _ => (None, None),
    };

    let fields = RoutedFields {
        name,
        lifetime_count,
        message_format: None,
        descriptor_path,
        dispatch_path,
        carrier_path: Some(carrier_tokens),
        sibling_scope: Some(sibling_scope.clone()),
    };
    route_item(&kind_ident, fields, out)
}

#[cfg(test)]
pub(crate) fn parse_wrapped_carrier_call_from_stream(
    tokens: TokenStream2,
    out: &mut ParsedInput,
) -> syn::Result<()> {
    syn::parse::Parser::parse2(
        |input: ParseStream<'_>| parse_wrapped_carrier_call(input, out),
        tokens,
    )
}

/// Build `<prefix>::<ident>`. A `None` prefix (same-crate bare carrier
/// hoisted to the crate root by `#[macro_export]`) becomes `crate::`.
fn join_path(prefix: Option<&TokenStream2>, ident: &Ident) -> TokenStream2 {
    if let Some(p) = prefix {
        quote::quote!(#p::#ident)
    } else {
        quote::quote!(crate::#ident)
    }
}

/// Per-item fields recovered from a carrier tuple or carrier call, routed by
/// [`route_item`] into an [`ItemInput`] or a [`DefinitionInput`].
#[derive(Default)]
struct RoutedFields {
    name: String,
    lifetime_count: usize,
    message_format: Option<String>,
    descriptor_path: Option<TokenStream2>,
    dispatch_path: Option<TokenStream2>,
    carrier_path: Option<TokenStream2>,
    sibling_scope: Option<TokenStream2>,
}

impl RoutedFields {
    fn into_item(self, kind: ItemKind) -> ItemInput {
        ItemInput {
            kind,
            name: self.name,
            lifetime_count: self.lifetime_count,
            message_format: self.message_format,
            descriptor_path: self.descriptor_path,
            dispatch_path: self.dispatch_path,
            sibling_scope: self.sibling_scope,
        }
    }

    fn into_definition(self, kind: DefinitionKind) -> DefinitionInput {
        DefinitionInput {
            kind,
            name: self.name,
            value_or_format: self.message_format.unwrap_or_default(),
            carrier_path: self.carrier_path,
            sibling_scope: self.sibling_scope,
        }
    }
}

/// Route one item into [`ParsedInput::items`] or [`ParsedInput::definitions`]
/// based on its kind ident.
fn route_item(kind_ident: &Ident, fields: RoutedFields, out: &mut ParsedInput) -> syn::Result<()> {
    match kind_ident.to_string().as_str() {
        "command" => out.items.push(fields.into_item(ItemKind::Command)),
        "reply" => out.items.push(fields.into_item(ItemKind::Reply)),
        "output" => out.items.push(fields.into_item(ItemKind::Output)),
        "constant" => out
            .definitions
            .push(fields.into_definition(DefinitionKind::Constant)),
        "enumeration" => out
            .definitions
            .push(fields.into_definition(DefinitionKind::Enumeration)),
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

/// Split a `<kind>_<name>` tail string into `(kind, name)` if `<kind>` is a
/// known carrier kind. Returns `None` for unknown kinds.
///
/// Reply and output carriers may encode a lifetime count as `lt<N>_` after
/// the kind: `reply_lt1_FooReply` means a `#[klipper_reply]` struct with
/// one lifetime parameter. The infix is left in `name`; callers recover it
/// via [`split_name_with_lifetime_count`].
fn split_kind_and_name(tail: &str) -> Option<(String, String)> {
    for kind in ["command", "reply", "output", "constant", "enumeration"] {
        if let Some(rest) = tail.strip_prefix(kind) {
            let Some(name) = rest.strip_prefix('_') else {
                continue;
            };
            if !name.is_empty() {
                return Some((kind.to_string(), name.to_string()));
            }
        }
    }
    None
}

/// Strip a leading `lt<N>_` segment from a reply/output item name, returning
/// `(base_name, lifetime_count)`. When no prefix is present the count is
/// `0` and the name is returned verbatim.
///
/// Example: `"lt1_FooReply"` → `("FooReply", 1)`.
pub(crate) fn split_name_with_lifetime_count(name: &str) -> (String, usize) {
    if let Some(rest) = name.strip_prefix("lt") {
        if let Some(underscore) = rest.find('_') {
            let (count_str, after) = rest.split_at(underscore);
            if let Ok(count) = count_str.parse::<usize>() {
                return (after[1..].to_string(), count);
            }
        }
    }
    (name.to_string(), 0)
}

fn path_to_tokens(path: &Path) -> TokenStream2 {
    quote::ToTokens::to_token_stream(path)
}

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
            "carrier calls carry no message_format; the dictionary reads \
             the sibling format const"
        );
        assert_eq!(parsed.items[1].kind, ItemKind::Reply);
        assert_eq!(parsed.items[1].name, "PingReply");
    }

    #[test]
    fn rejects_malformed_carrier_ident() {
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

#[cfg(test)]
mod def_sibling_scope_tests {
    #[test]
    fn route_item_populates_sibling_scope() {
        use proc_macro2::Span;
        let mut out = super::ParsedInput::default();
        let fields = super::RoutedFields {
            name: "foo".to_string(),
            dispatch_path: Some(quote::quote!($crate::sub::__ankyra_dispatch_foo)),
            carrier_path: Some(quote::quote!($crate::__ankyra_item_command_foo)),
            sibling_scope: Some(quote::quote!($crate::sub)),
            ..super::RoutedFields::default()
        };
        super::route_item(
            &syn::Ident::new("command", Span::call_site()),
            fields,
            &mut out,
        )
        .unwrap();
        assert_eq!(out.items.len(), 1);
        let mp = out.items[0].sibling_scope.as_ref().expect("prefix set");
        assert_eq!(mp.to_string().replace(' ', ""), "$crate::sub");
    }

    #[test]
    fn parse_wrapped_tuple_with_empty_prefix_promotes_to_dollar_crate() {
        let tokens: proc_macro2::TokenStream =
            "{ prefix: (), $crate::__ankyra_item_command_foo!() }"
                .parse()
                .expect("valid token stream");
        let mut out = super::ParsedInput::default();
        super::parse_wrapped_carrier_call_from_stream(tokens, &mut out).expect("parses");
        assert_eq!(out.items.len(), 1);
        let scope = out.items[0]
            .sibling_scope
            .as_ref()
            .expect("empty prefix must be promoted to $crate");
        assert_eq!(scope.to_string().replace(' ', ""), "$crate");
    }

    #[test]
    fn parse_wrapped_tuple_with_path_prefix() {
        let tokens: proc_macro2::TokenStream =
            "{ prefix: ($crate::sub), $crate::__ankyra_item_command_foo!() }"
                .parse()
                .expect("valid token stream");
        let mut out = super::ParsedInput::default();
        super::parse_wrapped_carrier_call_from_stream(tokens, &mut out).expect("parses");
        assert_eq!(out.items.len(), 1);
        let prefix = out.items[0]
            .sibling_scope
            .as_ref()
            .expect("wrapped form has prefix");
        assert_eq!(prefix.to_string().replace(' ', ""), "$crate::sub");
    }
}
