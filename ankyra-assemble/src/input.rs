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

use proc_macro2::TokenStream as TokenStream2;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Ident, LitStr, Path, Token, braced, bracketed, parenthesized};

use crate::sort::{ItemInput, ItemKind};

/// A `constant`- or `enumeration`-kind carrier tuple as delivered by the
/// macro crates. Task 10 does not consume this; Task 12 / D1 read it when
/// emitting the data dictionary.
#[derive(Debug, Clone)]
pub(crate) struct DefinitionInput {
    pub kind: DefinitionKind,
    pub name: String,
    pub value_or_format: String,
    /// Path to the `__ankyra_item_<kind>_<name>!` carrier macro. D1
    /// invokes `<path>!(name)` and `<path>!(value)` inside
    /// `const_format::concatcp!` so the dictionary's `config` and
    /// `enumerations` sections pick up authoritative values at
    /// const-eval time.
    pub carrier_path: Option<TokenStream2>,
    /// Effective module scope where this definition's sibling
    /// `__ANKYRA_VALUE_<kind>_<name>` / `__ANKYRA_NAME_<kind>_<name>` consts
    /// live. Populated for every carrier-backed definition:
    /// `Some($crate)` for crate-root items, `Some($crate::submod)` for
    /// submodule items. `None` only for inline-tuple test fixtures that
    /// carry no carrier at all.
    ///
    /// Consumed by `dictionary::push_definition_name` and
    /// `dictionary::push_definition_value` to construct
    /// `<scope>::__ANKYRA_<KIND>_<name>` directly, avoiding the fragile
    /// parse-path-and-rewrite-last-segment strategy this field replaced.
    pub sibling_scope: Option<TokenStream2>,
}

impl DefinitionInput {
    /// Convenience accessor: the kind tag string (e.g. `"constant"`) used
    /// when constructing sibling-const idents like
    /// `__ANKYRA_VALUE_constant_FOO`.
    pub(crate) fn kind_tag(&self) -> &'static str {
        self.kind.tag()
    }
}

/// Kind discriminant for definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefinitionKind {
    Constant,
    Enumeration,
}

impl DefinitionKind {
    /// Stable `&'static str` tag matching the carrier tuple kind ident
    /// (`constant` / `enumeration`). Used by the dictionary builder to
    /// construct sibling-const idents like `__ANKYRA_VALUE_constant_FOO`.
    pub(crate) fn tag(self) -> &'static str {
        match self {
            Self::Constant => "constant",
            Self::Enumeration => "enumeration",
        }
    }
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
            // The four dictionary-trailer metadata overrides. Stored as
            // raw `TokenStream2` so any `&'static str`-valued expression
            // (string literal, `env!`, `concat!`, module-path constant)
            // rides through unchanged; the type constraint is enforced
            // at dictionary-emit time by binding the expression to a
            // `pub const METADATA: &'static str` before splicing it
            // into `concatcp!`.
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
        } else if input.peek(syn::token::Brace) {
            parse_wrapped_carrier_call(input, out)?;
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
        None, // sibling_scope — inline tuple stays crate-root
        out,
    )
}

/// Parse one unexpanded carrier macro call `<path>::__ankyra_item_<kind>_<name>!()`
/// from the fold accumulator.
///
/// The carrier macro path serves double duty: its last segment encodes the
/// item's kind and name, and its prefix points back at the defining crate's
/// root (because every carrier macro is `#[macro_export]`). Task 12 uses the
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
/// the prefix is empty and Task 12 synthesizes a `crate::` qualifier.
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
    // For reply/output carriers the raw name may include an `lt<N>_`
    // prefix encoding the struct's lifetime parameter count (emitted by
    // `shared::carrier_ident_with_lifetimes`). Strip it and record the
    // count so `senders::emit` can synthesise a matching
    // `impl<'a0, ..> SendReply<Struct<'a0, ..>> for Sender` header.
    let (name, lifetime_count) = match kind_str.as_str() {
        "reply" | "output" => split_name_with_lifetime_count(&raw_name),
        _ => (raw_name, 0usize),
    };

    let kind_ident = Ident::new(&kind_str, last.ident.span());

    // Build the path prefix ("everything but the last segment"). This gives
    // Task 12 a handle for reconstructing the dispatch fn, descriptor fn,
    // and struct type at the carrier's defining scope.
    let mut prefix = Path {
        leading_colon: path.leading_colon,
        segments: Punctuated::default(),
    };
    let segs: Vec<_> = path.segments.iter().cloned().collect();
    let last_idx = segs.len() - 1;
    for (i, seg) in segs.into_iter().enumerate() {
        if i < last_idx {
            prefix.segments.push(seg);
        }
    }
    let prefix_tokens = if prefix.segments.is_empty() && prefix.leading_colon.is_none() {
        None
    } else {
        Some(quote::ToTokens::to_token_stream(&prefix))
    };

    // Build the kind-specific companion paths. For commands the dispatch
    // fn lives at `<prefix>::__ankyra_dispatch_<name>`. For replies/outputs
    // the descriptor fn lives at `<prefix>::__ankyra_descriptor_<name>`.
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

    // The carrier's format string is not accessible from a proc-macro
    // (the carrier `macro_rules!` has not expanded yet at this point).
    // D1 threads the full carrier macro path through so the dictionary
    // builder can invoke `<path>!(name)` / `<path>!(format)` in an
    // expression position inside `const_format::concatcp!` — rustc
    // expands the carrier at that position and the stitched dictionary
    // becomes a real compile-time string constant.
    let carrier_tokens = Some(quote::ToTokens::to_token_stream(&path));
    // For bare carrier calls we derive the sibling scope from the carrier
    // macro's own path (dropping the trailing `__ankyra_item_*` segment).
    // This is the legacy (test-only) path; production code goes through
    // `parse_wrapped_carrier_call` which threads a provider-supplied scope
    // directly. When the derived prefix is empty (crate-local bare call),
    // fall back to `$crate` so downstream sibling-const construction has
    // a scope to anchor to.
    let sibling_scope: Option<TokenStream2> = if prefix_tokens.is_some() {
        prefix_tokens.clone()
    } else {
        Some(quote::quote!($crate))
    };
    route_item_tokens(
        &kind_ident,
        name,
        lifetime_count,
        None,
        descriptor_path,
        dispatch_path,
        carrier_tokens,
        sibling_scope.as_ref(),
        out,
    )
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

    // `prefix:` label.
    let label: Ident = body.parse()?;
    if label != "prefix" {
        return Err(syn::Error::new(
            label.span(),
            format!("expected `prefix:` in wrapped carrier tuple, got `{label}`"),
        ));
    }
    let _colon: Token![:] = body.parse()?;

    // `(<prefix_tokens>)` — parse as a parenthesised token stream so an
    // empty `()` is legal.
    let prefix_body;
    parenthesized!(prefix_body in body);
    let prefix_ts: TokenStream2 = prefix_body.parse()?;
    // Empty `()` prefix means the item lives at the provider-defining
    // crate's root. Promote that to `$crate` so every carrier-backed item
    // has an explicit sibling scope — the dictionary builder requires one
    // and the fragile carrier-path trailing-segment rewrite is gone.
    let sibling_scope: Option<TokenStream2> = if prefix_ts.is_empty() {
        Some(quote::quote!($crate))
    } else {
        Some(prefix_ts)
    };

    let _comma: Token![,] = body.parse()?;

    // Carrier macro call — same extraction as parse_carrier_call, but
    // thread `sibling_scope` into route_item_tokens.
    parse_carrier_call_with_prefix(&body, sibling_scope.as_ref(), out)?;

    // Tolerate a trailing comma inside the braces.
    let _ = body.parse::<Token![,]>();
    Ok(())
}

/// Shared helper used by both `parse_carrier_call` (prefix = None) and
/// `parse_wrapped_carrier_call` (prefix = user-supplied).
///
/// The key behaviour difference: when `sibling_scope` is supplied, it is
/// used verbatim as the sibling-path prefix. Otherwise the prefix is
/// derived from the carrier macro's own path (stripping the trailing
/// `__ankyra_item_<kind>_<name>` segment) — which works for
/// crate-root-hoisted macros but would be empty for wrapped-form
/// carriers because `#[macro_export]` hoists the carrier ident to the
/// crate root in both cases.
///
/// The carrier macro path is consumed as a raw `TokenStream2` (by
/// collecting tokens up to the `!`) rather than as a `syn::Path`. This
/// lets the parser accept both fully-qualified paths (`::krate::…`) and
/// `$crate::…` paths — the latter appear verbatim in `proc_macro2`
/// token streams created from string literals (as used in unit tests),
/// while in real proc-macro invocations `$crate` has already been
/// resolved to a concrete path by rustc before the tokens reach us.
#[allow(clippy::too_many_lines)]
fn parse_carrier_call_with_prefix(
    input: ParseStream<'_>,
    sibling_scope: Option<&TokenStream2>,
    out: &mut ParsedInput,
) -> syn::Result<()> {
    // Collect all token trees until we hit the `!` that opens the macro
    // argument list. This avoids `syn::Path::parse` which rejects `$crate`.
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

    // The last ident in `path_tts` encodes the carrier kind and name.
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

    // Sibling-path prefix:
    //   - If the wrapped form supplied one, use it verbatim.
    //   - Otherwise, derive from the carrier macro's own path token trees
    //     (dropping everything from the last `::` separator onward).
    let sibling_prefix: Option<TokenStream2> = if sibling_scope.is_some() {
        sibling_scope.cloned()
    } else {
        // Find the index of the last ident in path_tts (the __ankyra_item_…
        // ident itself) and take everything before it, stripping trailing
        // `::` separators.
        let last_ident_pos = path_tts
            .iter()
            .rposition(|tt| matches!(tt, proc_macro2::TokenTree::Ident(_)));
        if let Some(pos) = last_ident_pos {
            // Everything before the last ident. Drop trailing punctuation
            // (the `::` separators are two consecutive Punct tokens).
            let prefix_tts: Vec<_> = path_tts[..pos].to_vec();
            // Strip trailing Punct tokens (the `::` separator before the ident).
            let prefix_tts: Vec<_> = prefix_tts
                .into_iter()
                .rev()
                .skip_while(|tt| matches!(tt, proc_macro2::TokenTree::Punct(_)))
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            if prefix_tts.is_empty() {
                None
            } else {
                let ts: TokenStream2 = prefix_tts.into_iter().collect();
                Some(ts)
            }
        } else {
            None
        }
    };

    let carrier_tokens: TokenStream2 = path_tts.into_iter().collect();
    let carrier_tokens = Some(carrier_tokens);

    let (descriptor_path, dispatch_path) = match kind_str.as_str() {
        "command" => {
            let dispatch_ident =
                Ident::new(&format!("__ankyra_dispatch_{name}"), last_ident.span());
            (
                None,
                Some(join_path(sibling_prefix.as_ref(), &dispatch_ident)),
            )
        }
        "reply" | "output" => {
            let desc_ident = Ident::new(&format!("__ankyra_descriptor_{name}"), last_ident.span());
            (Some(join_path(sibling_prefix.as_ref(), &desc_ident)), None)
        }
        _ => (None, None),
    };

    route_item_tokens(
        &kind_ident,
        name,
        lifetime_count,
        None,
        descriptor_path,
        dispatch_path,
        carrier_tokens,
        sibling_scope,
        out,
    )
}

/// Test helper: drive `parse_wrapped_carrier_call` from a standalone
/// token stream. Production callers go through `Parse::parse` on
/// `ParsedInput`.
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

/// Build a path `<prefix>::<ident>` as a token stream. When `prefix` is
/// `None` (same-crate bare carrier that `#[macro_export]` hoisted to the
/// crate root), prepend `crate::` so the emitted path resolves at the
/// firmware crate's root — which is where `#[klipper_*]` items sit in
/// v0.1.
fn join_path(prefix: Option<&TokenStream2>, ident: &Ident) -> TokenStream2 {
    if let Some(p) = prefix {
        quote::quote!(#p::#ident)
    } else {
        quote::quote!(crate::#ident)
    }
}

/// Route one item into [`ParsedInput::items`] or [`ParsedInput::definitions`]
/// when the descriptor and dispatch paths have been reconstructed
/// separately (carrier-call parse path).
#[allow(clippy::too_many_arguments)]
fn route_item_tokens(
    kind_ident: &Ident,
    name: String,
    lifetime_count: usize,
    message_format: Option<String>,
    descriptor_path: Option<TokenStream2>,
    dispatch_path: Option<TokenStream2>,
    carrier_path: Option<TokenStream2>,
    sibling_scope: Option<&TokenStream2>,
    out: &mut ParsedInput,
) -> syn::Result<()> {
    match kind_ident.to_string().as_str() {
        "command" => out.items.push(ItemInput {
            kind: ItemKind::Command,
            name,
            lifetime_count,
            message_format,
            descriptor_path,
            dispatch_path,
            sibling_scope: sibling_scope.cloned(),
        }),
        "reply" => out.items.push(ItemInput {
            kind: ItemKind::Reply,
            name,
            lifetime_count,
            message_format,
            descriptor_path,
            dispatch_path,
            sibling_scope: sibling_scope.cloned(),
        }),
        "output" => out.items.push(ItemInput {
            kind: ItemKind::Output,
            name,
            lifetime_count,
            message_format,
            descriptor_path,
            dispatch_path,
            sibling_scope: sibling_scope.cloned(),
        }),
        "constant" => out.definitions.push(DefinitionInput {
            kind: DefinitionKind::Constant,
            name,
            value_or_format: message_format.unwrap_or_default(),
            carrier_path,
            sibling_scope: sibling_scope.cloned(),
        }),
        "enumeration" => out.definitions.push(DefinitionInput {
            kind: DefinitionKind::Enumeration,
            name,
            value_or_format: message_format.unwrap_or_default(),
            carrier_path,
            sibling_scope: sibling_scope.cloned(),
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

/// Split a `<kind>_<name>` tail string into `(kind, name)` if `<kind>` is a
/// known carrier kind. Returns `None` for unknown kinds.
///
/// Reply and output carriers may encode a lifetime count as `lt<N>_` after
/// the kind: `reply_lt1_FooReply` means a `#[klipper_reply]` struct with
/// one lifetime parameter. The count is stripped here and returned in the
/// `name` unchanged; callers that care recover it via
/// [`split_name_with_lifetime_count`].
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

/// Route one item into [`ParsedInput::items`] or [`ParsedInput::definitions`]
/// based on its kind ident. Shared between the inline-tuple and
/// carrier-call parse paths so dispatch logic stays single-sourced.
fn route_item(
    kind_ident: &Ident,
    name: String,
    message_format: Option<String>,
    path_tokens: Option<TokenStream2>,
    sibling_scope: Option<&TokenStream2>,
    out: &mut ParsedInput,
) -> syn::Result<()> {
    match kind_ident.to_string().as_str() {
        "command" => out.items.push(ItemInput {
            kind: ItemKind::Command,
            name,
            lifetime_count: 0,
            message_format,
            descriptor_path: None,
            dispatch_path: path_tokens,
            sibling_scope: sibling_scope.cloned(),
        }),
        "reply" => out.items.push(ItemInput {
            kind: ItemKind::Reply,
            name,
            lifetime_count: 0,
            message_format,
            descriptor_path: path_tokens,
            dispatch_path: None,
            sibling_scope: sibling_scope.cloned(),
        }),
        "output" => out.items.push(ItemInput {
            kind: ItemKind::Output,
            name,
            lifetime_count: 0,
            message_format,
            descriptor_path: path_tokens,
            dispatch_path: None,
            sibling_scope: sibling_scope.cloned(),
        }),
        "constant" => out.definitions.push(DefinitionInput {
            kind: DefinitionKind::Constant,
            name,
            value_or_format: message_format.unwrap_or_default(),
            carrier_path: None,
            sibling_scope: sibling_scope.cloned(),
        }),
        "enumeration" => out.definitions.push(DefinitionInput {
            kind: DefinitionKind::Enumeration,
            name,
            value_or_format: message_format.unwrap_or_default(),
            carrier_path: None,
            sibling_scope: sibling_scope.cloned(),
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

#[cfg(test)]
mod def_sibling_scope_tests {
    use super::{DefinitionInput, DefinitionKind};

    #[test]
    fn definition_input_carries_sibling_scope() {
        let d = DefinitionInput {
            kind: DefinitionKind::Constant,
            name: "FOO".into(),
            value_or_format: "1".into(),
            carrier_path: Some(quote::quote!($crate::__ankyra_item_constant_FOO)),
            sibling_scope: Some(quote::quote!($crate::sub)),
        };
        assert!(d.sibling_scope.is_some());
    }

    #[test]
    fn route_item_tokens_populates_sibling_scope() {
        use proc_macro2::Span;
        let mut out = super::ParsedInput::default();
        let prefix = Some(quote::quote!($crate::sub));
        super::route_item_tokens(
            &syn::Ident::new("command", Span::call_site()),
            "foo".to_string(),
            0,
            None,
            None,
            Some(quote::quote!($crate::sub::__ankyra_dispatch_foo)),
            Some(quote::quote!($crate::__ankyra_item_command_foo)),
            prefix.as_ref(),
            &mut out,
        )
        .unwrap();
        assert_eq!(out.items.len(), 1);
        let mp = out.items[0].sibling_scope.as_ref().expect("prefix set");
        assert_eq!(mp.to_string().replace(' ', ""), "$crate::sub");
    }

    #[test]
    fn parse_wrapped_tuple_with_empty_prefix_promotes_to_dollar_crate() {
        // Back-compat: wrapped tuples emitted before Task C7 passed an empty
        // `()` prefix for crate-root items. The parser now promotes that to
        // `$crate` so every carrier-backed item carries an explicit sibling
        // scope — the dictionary builder no longer tolerates a missing one.
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
