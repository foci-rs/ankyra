//! `ankyra_provider!` and `ankyra_reexport_provider!` function-like proc-macros.
//!
//! # `ankyra_provider!`
//!
//! Shape:
//!
//! ```ignore
//! ankyra_provider! {
//!     name: CORE_PROVIDER,
//!     commands: [emergency_stop, get_clock],
//!     replies: [PingReply, Pong],
//!     outputs: [DebugPrint],
//!     constants: [CLOCK_FREQ],
//!     enumerations: [MotorKind],
//! }
//! ```
//!
//! Only `name:` is required; every list defaults to an empty `[]` when its
//! key is omitted. Unknown keys cause a span-pointed parse error. Idents
//! within a single list must be unique — duplicates are reported at the
//! second occurrence's span. Cross-list collisions (an ident appearing in
//! both `commands` and `replies`, say) are left to the assembler;
//! at the provider macro level we only police per-list shape.
//!
//! The macro emits three surfaces for downstream consumers:
//!
//! 1. A hidden zero-sized type `__ankyra_provider_ty_<NAME>` whose
//!    [`ProviderSpec`](../../../ankyra/provider/trait.ProviderSpec.html)
//!    impl inlines every descriptor call site. For replies, outputs,
//!    constants, and enumerations, the slice initialisers call the
//!    `pub const fn __ankyra_descriptor_<T>()` that the item-level macros
//!    emit. Commands synthesize a `MessageDescriptor::command(name, name)`
//!    inline — the per-command message format carried by the provider
//!    surface is deliberately a placeholder here; the assembler
//!    rebuilds the authoritative `message_format` from the command
//!    carrier tuples when building the Klipper data dictionary. The
//!    `ProviderSpec` slices are intended for `ProviderRef::new::<P>()`
//!    consumers that care about counts and item kinds — not wire format.
//! 2. A user-facing `pub const <NAME>: ProviderRef = ProviderRef::new::<...>();`.
//!    This is what crates invoke by referring to `CORE_PROVIDER` in their
//!    own `ankyra_config!` entries.
//! 3. A `#[macro_export] macro_rules! __ankyra_provider_<NAME>` that plays
//!    the role of a continuation in the `ankyra_config!` CPS fold. Its body hands
//!    off to `::ankyra::__ankyra_fold_providers!` with the carrier
//!    macro invocations for every item in the provider appended to the
//!    accumulator. The carrier macros are referenced through `$crate::` so
//!    they resolve in the provider-defining crate — `#[macro_export]`
//!    publishes each carrier at that crate's root, so the path remains
//!    valid even when `ankyra_provider!` sits in a submodule.
//!
//! # `ankyra_reexport_provider!`
//!
//! Shape: `ankyra_reexport_provider!(upstream_crate::nested::PROVIDER_NAME);`
//!
//! Emits two `pub use` lines: one for the user-facing const and one for
//! the companion macro. `#[macro_export]` publishes the companion macro at
//! the defining crate's root regardless of how deep the `ankyra_provider!`
//! invocation sits, which is why the re-export path for the companion
//! macro drops the intermediate path segments — see
//! `crate::shared::provider_path_to_companion`.

use proc_macro::TokenStream;
use proc_macro_error2::abort;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Error, Ident, Path, Token, bracketed, parse_macro_input};

use crate::shared::{
    carrier_ident, carrier_ident_with_lifetimes, descriptor_ident, provider_companion_ident,
};

/// Parsed path argument for a `ankyra_provider!` item list entry.
///
/// Accepts two shapes:
/// - A bare ident (`foo`) — item lives at the provider-defining crate's root.
/// - A `crate::…`-prefixed path (`crate::klipper_mod::foo`) — item lives in a
///   submodule of the provider-defining crate.
///
/// Cross-crate paths and `::foo`-style absolute paths are rejected so the
/// same-crate / cross-crate split (see `ankyra_reexport_provider!`)
/// stays enforced at one layer.
#[derive(Debug, Clone)]
pub(crate) struct ProviderPath {
    path: syn::Path,
}

impl ProviderPath {
    /// Leaf (last) segment's ident — the `#[klipper_*]` item's own name.
    /// This is the protocol-facing name on the wire and the source of the
    /// `#[macro_export]` carrier ident.
    pub(crate) fn leaf_ident(&self) -> &Ident {
        &self
            .path
            .segments
            .last()
            .expect("ProviderPath invariant: at least one segment")
            .ident
    }

    /// Number of lifetime arguments on the leaf segment.
    ///
    /// `ankyra_provider!` accepts lifetime-only generic arguments on
    /// reply/output entries so the provider can thread lifetime-count
    /// information into the carrier call tokens the assembler parses.
    /// For an entry like `crate::replies::FociTraceData<'_>` this returns
    /// `1`; for plain `crate::replies::Clock` it returns `0`.
    pub(crate) fn leaf_lifetime_count(&self) -> usize {
        let last = self
            .path
            .segments
            .last()
            .expect("ProviderPath invariant: at least one segment");
        match &last.arguments {
            syn::PathArguments::AngleBracketed(ab) => ab
                .args
                .iter()
                .filter(|a| matches!(a, syn::GenericArgument::Lifetime(_)))
                .count(),
            _ => 0,
        }
    }

    /// Prefix tokens suitable for splicing into the provider companion
    /// macro's wrapper tuple. `None` for a bare ident (item lives at the
    /// crate root); `Some($crate::a::b)` for a multi-segment path (the
    /// leading `crate` segment is rewritten to `$crate` so the tokens
    /// resolve relative to the provider-defining crate in both same-crate
    /// and cross-crate `ankyra_config!` contexts).
    pub(crate) fn prefix_tokens(&self) -> Option<TokenStream2> {
        if self.path.segments.len() < 2 {
            return None;
        }
        let last_idx = self.path.segments.len() - 1;
        let rewritten: Vec<TokenStream2> = self
            .path
            .segments
            .iter()
            .take(last_idx)
            .enumerate()
            .map(|(i, seg)| {
                let ident = &seg.ident;
                if i == 0 && ident == "crate" {
                    quote::quote!($crate)
                } else {
                    quote::quote!(#ident)
                }
            })
            .collect();
        Some(quote::quote! { #(#rewritten)::* })
    }

    /// Expose the underlying `syn::Path` for rendering into error messages.
    /// Used by `validate_unique` to cite both sides of a duplicate-leaf
    /// collision in the error message.
    pub(crate) fn as_path(&self) -> &syn::Path {
        &self.path
    }
}

impl syn::parse::Parse for ProviderPath {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let path: syn::Path = input.parse()?;

        // Reject `::foo::bar` — an absolute path with leading colons.
        if path.leading_colon.is_some() {
            return Err(syn::Error::new_spanned(
                &path,
                "`ankyra_provider!` item paths must start with `crate::` or be a \
                 bare ident; absolute paths with leading `::` are not supported. \
                 Cross-crate items must go through `ankyra_reexport_provider!`.",
            ));
        }

        // Reject multi-segment paths whose first segment is not `crate`.
        if path.segments.len() > 1 && path.segments[0].ident != "crate" {
            return Err(syn::Error::new_spanned(
                &path,
                "`ankyra_provider!` item paths must start with `crate::` or be a \
                 bare ident; cross-crate items must go through \
                 `ankyra_reexport_provider!`.",
            ));
        }

        // Reject generic arguments / turbofish at every segment except the
        // leaf, which is allowed to carry lifetime-only generic args (e.g.
        // `crate::replies::FociTraceData<'_>`) so the provider can thread
        // the lifetime count through to the assembler. Type and const
        // generic arguments remain rejected everywhere — the assembler has
        // no way to substitute a concrete type for `T` when emitting the
        // `SendReply` impl.
        let last_idx = path.segments.len() - 1;
        for (i, seg) in path.segments.iter().enumerate() {
            if i < last_idx {
                if !matches!(seg.arguments, syn::PathArguments::None) {
                    return Err(syn::Error::new_spanned(
                        seg,
                        "`ankyra_provider!` item paths must be plain paths; generic \
                         arguments on non-leaf segments are not supported",
                    ));
                }
                continue;
            }
            match &seg.arguments {
                syn::PathArguments::None => {}
                syn::PathArguments::AngleBracketed(ab) => {
                    for arg in &ab.args {
                        match arg {
                            syn::GenericArgument::Lifetime(_) => {}
                            other => {
                                return Err(syn::Error::new_spanned(
                                    other,
                                    "`ankyra_provider!` reply/output leaf paths \
                                     may only carry lifetime arguments; type \
                                     and const generics are not supported",
                                ));
                            }
                        }
                    }
                }
                syn::PathArguments::Parenthesized(_) => {
                    return Err(syn::Error::new_spanned(
                        seg,
                        "`ankyra_provider!` item paths do not accept Fn-style \
                         parenthesized generic arguments",
                    ));
                }
            }
        }

        Ok(Self { path })
    }
}

/// Parsed shape of `ankyra_provider! { key: value, ... }`.
///
/// Every list defaults to empty; `name:` is the only required key.
struct ProviderInput {
    name: Ident,
    commands: Vec<ProviderPath>,
    replies: Vec<ProviderPath>,
    outputs: Vec<ProviderPath>,
    constants: Vec<ProviderPath>,
    enumerations: Vec<ProviderPath>,
}

impl Parse for ProviderInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut name: Option<Ident> = None;
        let mut commands: Option<(proc_macro2::Span, Vec<ProviderPath>)> = None;
        let mut replies: Option<(proc_macro2::Span, Vec<ProviderPath>)> = None;
        let mut outputs: Option<(proc_macro2::Span, Vec<ProviderPath>)> = None;
        let mut constants: Option<(proc_macro2::Span, Vec<ProviderPath>)> = None;
        let mut enumerations: Option<(proc_macro2::Span, Vec<ProviderPath>)> = None;

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            let _colon: Token![:] = input.parse()?;
            match key.to_string().as_str() {
                "name" => {
                    if name.is_some() {
                        return Err(Error::new(key.span(), "duplicate `name` key"));
                    }
                    let value: Ident = input.parse()?;
                    name = Some(value);
                }
                list_key @ ("commands" | "replies" | "outputs" | "constants" | "enumerations") => {
                    let span = key.span();
                    let idents = parse_provider_path_list(input)?;
                    let slot = match list_key {
                        "commands" => &mut commands,
                        "replies" => &mut replies,
                        "outputs" => &mut outputs,
                        "constants" => &mut constants,
                        "enumerations" => &mut enumerations,
                        _ => unreachable!(),
                    };
                    if slot.is_some() {
                        return Err(Error::new(
                            key.span(),
                            format!("duplicate `{list_key}` key"),
                        ));
                    }
                    *slot = Some((span, idents));
                }
                other => {
                    return Err(Error::new(
                        key.span(),
                        format!(
                            "unknown ankyra_provider! key `{other}`; expected one of: \
                             name, commands, replies, outputs, constants, enumerations"
                        ),
                    ));
                }
            }
            if input.peek(Token![,]) {
                let _comma: Token![,] = input.parse()?;
            } else {
                break;
            }
        }

        let Some(name) = name else {
            return Err(Error::new(
                proc_macro2::Span::call_site(),
                "ankyra_provider! requires a `name:` key",
            ));
        };

        let commands = validate_unique(commands.map(|(_, v)| v).unwrap_or_default())?;
        let replies = validate_unique(replies.map(|(_, v)| v).unwrap_or_default())?;
        let outputs = validate_unique(outputs.map(|(_, v)| v).unwrap_or_default())?;
        let constants = validate_unique(constants.map(|(_, v)| v).unwrap_or_default())?;
        let enumerations = validate_unique(enumerations.map(|(_, v)| v).unwrap_or_default())?;

        Ok(Self {
            name,
            commands,
            replies,
            outputs,
            constants,
            enumerations,
        })
    }
}

/// Parse a `[path_or_ident, …]` list into a `Vec<ProviderPath>`. Trailing
/// commas and empty lists are both accepted. Element-level rejection of
/// malformed paths happens inside `ProviderPath::parse`.
fn parse_provider_path_list(input: ParseStream) -> syn::Result<Vec<ProviderPath>> {
    let body;
    let _brackets = bracketed!(body in input);
    let punct: Punctuated<ProviderPath, Token![,]> = Punctuated::parse_terminated(&body)?;
    Ok(punct.into_iter().collect())
}

/// Enforce per-list leaf-ident uniqueness. See the design rationale in
/// the spec's §2 (paths sharing a leaf produce duplicate
/// `#[macro_export]` carriers and duplicate wire names regardless).
fn validate_unique(entries: Vec<ProviderPath>) -> syn::Result<Vec<ProviderPath>> {
    let mut seen: std::collections::HashMap<String, syn::Path> =
        std::collections::HashMap::with_capacity(entries.len());
    for entry in &entries {
        let key = entry.leaf_ident().to_string();
        if let Some(first) = seen.get(&key) {
            let rendered_first = quote::quote!(#first).to_string().replace(' ', "");
            let p = entry.as_path();
            let rendered_second = quote::quote!(#p).to_string().replace(' ', "");
            let message = if rendered_first == rendered_second {
                format!(
                    "duplicate entry `{key}` in ankyra_provider! list; \
                     two `#[klipper_*]` items in one crate cannot share an \
                     ident. Rename one of them."
                )
            } else {
                format!(
                    "duplicate entry `{key}` in ankyra_provider! list \
                     (first: {rendered_first}, second: {rendered_second}); \
                     two `#[klipper_*]` items in one crate cannot share an \
                     ident. Rename one of them."
                )
            };
            return Err(syn::Error::new(entry.leaf_ident().span(), message));
        }
        seen.insert(key, entry.as_path().clone());
    }
    Ok(entries)
}

/// Entry point for `ankyra_provider! { ... }` expansion.
pub fn expand_provider(input: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(input as ProviderInput);
    expand_provider_impl(&parsed).into()
}

fn expand_provider_impl(p: &ProviderInput) -> TokenStream2 {
    let name = &p.name;
    let marker_ident = format_ident!("__ankyra_provider_ty_{}", name);
    let companion_ident = provider_companion_ident(name);

    // Commands synthesize MessageDescriptor inline; no descriptor fn call.
    // Protocol name is the leaf ident so submodule-registered commands
    // keep their short name on the wire.
    let message_entries: Vec<TokenStream2> = p
        .commands
        .iter()
        .map(|cmd| {
            let name_str = cmd.leaf_ident().to_string();
            quote! {
                ::ankyra::descriptor::MessageDescriptor::command(#name_str, #name_str),
            }
        })
        .collect();

    // Replies: call the pub const fn emitted by `#[klipper_reply]` at
    // whatever module the provider entry named. qualify_descriptor yields
    // a bare `__ankyra_descriptor_<Leaf>()` for bare idents or
    // `crate::submod::__ankyra_descriptor_<Leaf>()` for path entries.
    let reply_entries: Vec<TokenStream2> = p
        .replies
        .iter()
        .map(|reply| {
            let call = qualify_descriptor(reply);
            quote! { #call, }
        })
        .collect();

    let output_entries: Vec<TokenStream2> = p
        .outputs
        .iter()
        .map(|out| {
            let call = qualify_descriptor(out);
            quote! { #call, }
        })
        .collect();

    // Definitions: constants and enumerations share a descriptor shape,
    // so they're concatenated into one builder loop.
    let definition_entries: Vec<TokenStream2> = p
        .constants
        .iter()
        .chain(p.enumerations.iter())
        .map(|d| {
            let call = qualify_descriptor(d);
            quote! { #call, }
        })
        .collect();

    let provider_spec_impl = quote! {
        #[doc(hidden)]
        #[allow(non_camel_case_types)]
        pub struct #marker_ident;

        impl ::ankyra::provider::ProviderSpec for #marker_ident {
            const MESSAGES: &'static [::ankyra::descriptor::MessageDescriptor] = &[
                #(#message_entries)*
            ];
            const REPLIES: &'static [::ankyra::descriptor::ReplyDescriptor] = &[
                #(#reply_entries)*
            ];
            const OUTPUTS: &'static [::ankyra::descriptor::OutputDescriptor] = &[
                #(#output_entries)*
            ];
            const DEFINITIONS: &'static [::ankyra::descriptor::DefinitionDescriptor] = &[
                #(#definition_entries)*
            ];
        }
    };

    let user_const = quote! {
        pub const #name: ::ankyra::provider::ProviderRef =
            ::ankyra::provider::ProviderRef::new::<#marker_ident>();
    };

    // Build the CPS-fold companion macro body. Each item contributes one
    // carrier invocation line, grouped by kind in declaration order:
    // commands, replies, outputs, constants, enumerations.
    let carrier_calls: Vec<TokenStream2> = p
        .commands
        .iter()
        .map(|i| carrier_call("command", i))
        .chain(p.replies.iter().map(|i| carrier_call("reply", i)))
        .chain(p.outputs.iter().map(|i| carrier_call("output", i)))
        .chain(p.constants.iter().map(|i| carrier_call("constant", i)))
        .chain(
            p.enumerations
                .iter()
                .map(|i| carrier_call("enumeration", i)),
        )
        .collect();

    let companion_macro = quote! {
        #[doc(hidden)]
        #[macro_export]
        macro_rules! #companion_ident {
            (
                config = { $($cfg:tt)* },
                accumulator = [ $($acc:tt)* ],
                remaining = [ $($remaining:path),* $(,)? ],
            ) => {
                ::ankyra::__ankyra_fold_providers! {
                    config = { $($cfg)* },
                    accumulator = [
                        $($acc)*
                        #(#carrier_calls)*
                    ],
                    remaining = [ $($remaining),* ],
                }
            };
        }
    };

    quote! {
        #provider_spec_impl
        #user_const
        #companion_macro
    }
}

/// Build a qualified descriptor call site for a `ProviderSpec` entry.
///
/// Bare-ident entries yield `crate::__ankyra_descriptor_<Leaf>()`, making
/// them equivalent to `crate::<Leaf>` entries and ensuring the call always
/// resolves at the defining crate's root.
/// Path entries yield `crate::submod::__ankyra_descriptor_<Leaf>()`,
/// threading the user-written module prefix onto the descriptor call
/// so it resolves at the submodule where the descriptor actually lives.
///
/// Used by every `ProviderSpec` builder loop in `expand_provider_impl`
/// (replies, outputs, constants, enumerations). Commands do not call
/// descriptors — they synthesize `MessageDescriptor::command(name, name)`
/// inline — but they still use `leaf_ident()` via the caller to derive
/// the protocol name.
fn qualify_descriptor(entry: &ProviderPath) -> TokenStream2 {
    let leaf = entry.leaf_ident();
    let desc = descriptor_ident(leaf);
    // `$crate`-prefixed tokens work inside the companion macro body
    // but not inside `expand_provider_impl`'s direct `quote!` output,
    // which lives at the provider's declaration scope where `$crate`
    // does not mean anything. Strip the `$crate` back to `crate` for
    // this caller — the provider's `ProviderSpec` impl is always
    // emitted in the same crate as the items it references.
    if let Some(prefix) = entry.prefix_tokens() {
        let bare_crate_prefix = crate_prefix_for_provider_spec(&prefix);
        quote! { #bare_crate_prefix::#desc() }
    } else {
        quote! { crate::#desc() }
    }
}

/// Convert a prefix token stream that uses `$crate` (the form produced by
/// `ProviderPath::prefix_tokens` for companion-macro splicing) into a
/// plain `crate`-prefixed form suitable for inlining into
/// `expand_provider_impl`'s direct output.
fn crate_prefix_for_provider_spec(prefix_with_dollar_crate: &TokenStream2) -> TokenStream2 {
    let rendered = prefix_with_dollar_crate.to_string();
    let rewritten = rendered
        .replace("$ crate", "crate")
        .replace("$crate", "crate");
    syn::parse_str::<syn::Path>(&rewritten)
        .map_or_else(|_| prefix_with_dollar_crate.clone(), |p| quote!(#p))
}

/// Emit one wrapper-tuple accumulator entry for the companion macro body:
///
/// ```text
/// { prefix: (<prefix_tokens>), $crate::__ankyra_item_<kind>_<leaf>!() }
/// ```
///
/// `<prefix_tokens>` is empty for bare-ident entries (item lives at the
/// provider-defining crate's root) or `$crate::…` for path entries (with
/// the leading `crate` rewritten to `$crate` by
/// `ProviderPath::prefix_tokens`).
///
/// The assembler's `parse_wrapped_carrier_call` (Phase C) reads the
/// prefix syntactically and threads it into `ItemInput::module_prefix` /
/// `DefinitionInput::module_prefix` for sibling-path reconstruction.
fn carrier_call(kind: &str, entry: &ProviderPath) -> TokenStream2 {
    // `kind` determines whether lifetime-count encoding applies. Only
    // reply/output carriers pick up the `lt<N>_` infix from
    // `carrier_ident_with_lifetimes`; commands/constants/enumerations
    // always use the plain form.
    let lifetime_count = match kind {
        "reply" | "output" => entry.leaf_lifetime_count(),
        _ => 0,
    };
    let carrier = if lifetime_count == 0 {
        carrier_ident(kind, entry.leaf_ident())
    } else {
        carrier_ident_with_lifetimes(kind, entry.leaf_ident(), lifetime_count)
    };
    // Every carrier-backed item needs an explicit sibling scope on the
    // assembler side — crate-root items included. For a multi-segment
    // provider path, `prefix_tokens()` hands back `$crate::…`. For a bare
    // ident (item lives at the provider-defining crate's root), we emit
    // a plain `$crate` so the assembler's parser can thread that scope
    // into `ItemInput::sibling_scope` / `DefinitionInput::sibling_scope`
    // instead of parsing the carrier-macro path's trailing segment.
    let prefix_inner = entry.prefix_tokens().unwrap_or_else(|| quote!($crate));
    quote! {
        { prefix: (#prefix_inner), $crate::#carrier!() },
    }
}

/// Parsed `ankyra_reexport_provider!(upstream::PROVIDER_NAME)`.
struct ReexportInput {
    path: Path,
}

impl Parse for ReexportInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let path: Path = input.parse()?;
        // Allow a trailing comma for ergonomics.
        if input.peek(Token![,]) {
            let _: Token![,] = input.parse()?;
        }
        if !input.is_empty() {
            return Err(Error::new(
                input.span(),
                "ankyra_reexport_provider! takes a single provider path argument",
            ));
        }
        Ok(Self { path })
    }
}

/// Entry point for `ankyra_reexport_provider!(...)` expansion.
pub fn expand_reexport(input: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(input as ReexportInput);
    expand_reexport_impl(&parsed).into()
}

fn expand_reexport_impl(r: &ReexportInput) -> TokenStream2 {
    let provider_path = &r.path;
    let companion_path = match crate::shared::provider_path_to_companion(provider_path) {
        Ok(p) => p,
        Err(e) => abort!(provider_path, "{}", e),
    };
    quote! {
        pub use #provider_path;
        pub use #companion_path;
    }
}

#[cfg(test)]
mod provider_path_tests {
    use super::ProviderPath;
    use syn::parse_quote;

    #[test]
    fn parses_bare_ident() {
        let p: ProviderPath = parse_quote!(foo);
        assert_eq!(p.leaf_ident().to_string(), "foo");
        assert!(p.prefix_tokens().is_none(), "bare ident has no prefix");
    }

    #[test]
    fn parses_crate_path() {
        let p: ProviderPath = parse_quote!(crate::klipper_mod::get_clock);
        assert_eq!(p.leaf_ident().to_string(), "get_clock");
        let prefix = p
            .prefix_tokens()
            .expect("crate::a::b path must yield a prefix");
        assert_eq!(prefix.to_string().replace(' ', ""), "$crate::klipper_mod");
    }

    #[test]
    fn rejects_leading_colons() {
        let err = syn::parse_str::<ProviderPath>("::foo::bar").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("must start with `crate::`"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn rejects_extern_crate_path() {
        let err = syn::parse_str::<ProviderPath>("other_crate::foo").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("must start with `crate::`"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn rejects_turbofish() {
        let err = syn::parse_str::<ProviderPath>("foo::<T>").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("generic arguments")
                || msg.contains("turbofish")
                || msg.contains("lifetime arguments"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn rejects_generic_on_bare_ident() {
        let err = syn::parse_str::<ProviderPath>("Foo<T>").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("generic arguments")
                || msg.contains("turbofish")
                || msg.contains("lifetime arguments"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn accepts_lifetime_arg_on_leaf() {
        let p = syn::parse_str::<ProviderPath>("crate::replies::Borrowed<'_>")
            .expect("leaf lifetime arg must parse");
        assert_eq!(p.leaf_ident().to_string(), "Borrowed");
        assert_eq!(p.leaf_lifetime_count(), 1);
    }

    #[test]
    fn accepts_multiple_lifetime_args_on_leaf() {
        let p = syn::parse_str::<ProviderPath>("crate::replies::Multi<'_, '_>")
            .expect("multiple leaf lifetime args must parse");
        assert_eq!(p.leaf_lifetime_count(), 2);
    }

    #[test]
    fn leaf_lifetime_count_zero_for_plain_path() {
        let p: ProviderPath = syn::parse_quote!(crate::replies::Plain);
        assert_eq!(p.leaf_lifetime_count(), 0);
    }

    #[test]
    fn qualify_descriptor_for_bare_ident() {
        use syn::parse_quote;
        let p: ProviderPath = parse_quote!(Pong);
        let out = super::qualify_descriptor(&p).to_string();
        // Bare ident is equivalent to `crate::Pong`, so the descriptor
        // must also resolve through `crate::`.
        assert_eq!(out.replace(' ', ""), "crate::__ankyra_descriptor_Pong()");
    }

    #[test]
    fn qualify_descriptor_for_path() {
        use syn::parse_quote;
        let p: ProviderPath = parse_quote!(crate::submod::Pong);
        let out = super::qualify_descriptor(&p).to_string();
        assert_eq!(
            out.replace(' ', ""),
            "crate::submod::__ankyra_descriptor_Pong()"
        );
    }

    #[test]
    fn validate_unique_rejects_same_leaf_across_paths() {
        use syn::parse_quote;
        let a: ProviderPath = parse_quote!(crate::a::foo);
        let b: ProviderPath = parse_quote!(crate::b::foo);
        let err = super::validate_unique(vec![a, b]).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("duplicate entry `foo`"),
            "missing duplicate prefix: {msg}"
        );
        assert!(
            msg.contains("crate::a::foo") && msg.contains("crate::b::foo"),
            "missing path context: {msg}"
        );
    }

    #[test]
    fn validate_unique_accepts_distinct_leaves() {
        use syn::parse_quote;
        let a: ProviderPath = parse_quote!(crate::a::foo);
        let b: ProviderPath = parse_quote!(crate::b::bar);
        super::validate_unique(vec![a, b]).expect("distinct leaves pass");
    }

    #[test]
    fn validate_unique_rejects_duplicate_identical_paths() {
        use syn::parse_quote;
        let a: ProviderPath = parse_quote!(crate::a::foo);
        let b: ProviderPath = parse_quote!(crate::a::foo);
        let err = super::validate_unique(vec![a, b]).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("duplicate entry `foo`"),
            "missing duplicate prefix: {msg}"
        );
        // The identical-path branch suppresses the (first: …, second: …)
        // parenthetical — verify that.
        assert!(
            !msg.contains("(first:"),
            "identical-path dedup should omit the parenthetical: {msg}"
        );
    }

    #[test]
    fn provider_input_parses_mixed_entries() {
        use syn::parse_quote;
        let input: super::ProviderInput = parse_quote! {
            name: P,
            commands: [foo, crate::sub::bar],
        };
        assert_eq!(input.commands.len(), 2);
        assert_eq!(input.commands[0].leaf_ident().to_string(), "foo");
        assert_eq!(input.commands[1].leaf_ident().to_string(), "bar");
        assert!(input.commands[0].prefix_tokens().is_none());
        let bar_prefix = input.commands[1].prefix_tokens().unwrap();
        assert_eq!(bar_prefix.to_string().replace(' ', ""), "$crate::sub");
    }

    #[test]
    fn carrier_call_wraps_with_dollar_crate_prefix_for_bare_ident() {
        use syn::parse_quote;
        let entry: ProviderPath = parse_quote!(emergency_stop);
        let out = super::carrier_call("command", &entry).to_string();
        let normalised = out.replace(' ', "");
        // Expected shape:
        //   {prefix:($crate),$crate::__ankyra_item_command_emergency_stop!()},
        //
        // Emitting `$crate` (rather than an empty `()`) is the fix that
        // eliminates the dictionary builder's carrier-path trailing-segment
        // rewrite: every carrier-backed item now reaches the assembler
        // with an explicit sibling scope.
        assert!(
            normalised.contains("prefix:($crate)"),
            "expected $crate prefix block for bare-ident entry: {out}"
        );
        assert!(
            normalised.contains("$crate::__ankyra_item_command_emergency_stop!()"),
            "expected carrier call preserved: {out}"
        );
    }

    #[test]
    fn carrier_call_wraps_with_dollar_crate_prefix_for_path() {
        use syn::parse_quote;
        let entry: ProviderPath = parse_quote!(crate::sub::foo);
        let out = super::carrier_call("command", &entry).to_string();
        let normalised = out.replace(' ', "");
        assert!(
            normalised.contains("prefix:($crate::sub)"),
            "expected $crate-prefixed prefix: {out}"
        );
        assert!(
            normalised.contains("$crate::__ankyra_item_command_foo!()"),
            "expected carrier call with leaf ident: {out}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    fn render(ts: &TokenStream2) -> String {
        ts.to_string()
    }

    fn expand_for_test(input: TokenStream2) -> TokenStream2 {
        let parsed: ProviderInput = syn::parse2(input).expect("parse ProviderInput");
        expand_provider_impl(&parsed)
    }

    #[test]
    fn minimal_provider_emits_all_three_surfaces() {
        let input = quote! {
            name: CORE_PROVIDER,
            commands: [emergency_stop],
            replies: [PingReply],
        };
        let out = render(&expand_for_test(input));
        // Marker type.
        assert!(
            out.contains("pub struct __ankyra_provider_ty_CORE_PROVIDER"),
            "missing marker type: {out}"
        );
        // User-facing const.
        assert!(
            out.contains("pub const CORE_PROVIDER : :: ankyra :: provider :: ProviderRef"),
            "missing user const: {out}"
        );
        // ProviderSpec impl.
        assert!(
            out.contains(
                ":: ankyra :: provider :: ProviderSpec for __ankyra_provider_ty_CORE_PROVIDER"
            ),
            "missing ProviderSpec impl: {out}"
        );
        // Message descriptor inlined for the command.
        assert!(
            out.contains("MessageDescriptor :: command (\"emergency_stop\" , \"emergency_stop\")"),
            "missing command descriptor: {out}"
        );
        // Reply descriptor calls the item-level pub const fn.
        assert!(
            out.contains("__ankyra_descriptor_PingReply ()"),
            "missing reply descriptor call: {out}"
        );
        // Companion macro with carrier invocations.
        assert!(
            out.contains("macro_rules ! __ankyra_provider_CORE_PROVIDER"),
            "missing companion macro: {out}"
        );
        assert!(
            out.contains("$ crate :: __ankyra_item_command_emergency_stop ! ()"),
            "missing command carrier call: {out}"
        );
        assert!(
            out.contains("$ crate :: __ankyra_item_reply_PingReply ! ()"),
            "missing reply carrier call: {out}"
        );
    }

    #[test]
    fn all_lists_default_to_empty() {
        let input = quote! {
            name: EMPTY,
        };
        let out = render(&expand_for_test(input));
        // ProviderSpec impl has four empty slice initialisers.
        assert!(
            out.contains(
                "MESSAGES : & 'static [:: ankyra :: descriptor :: MessageDescriptor] = & []"
            ),
            "MESSAGES not empty: {out}"
        );
        assert!(
            out.contains("REPLIES : & 'static [:: ankyra :: descriptor :: ReplyDescriptor] = & []"),
            "REPLIES not empty: {out}"
        );
    }

    #[test]
    fn definitions_merge_constants_and_enumerations() {
        let input = quote! {
            name: DICT,
            constants: [CLOCK_FREQ],
            enumerations: [MotorKind],
        };
        let out = render(&expand_for_test(input));
        // Both descriptor fns appear in the DEFINITIONS slice initialiser.
        assert!(
            out.contains("__ankyra_descriptor_CLOCK_FREQ ()"),
            "missing constant desc: {out}"
        );
        assert!(
            out.contains("__ankyra_descriptor_MotorKind ()"),
            "missing enumeration desc: {out}"
        );
        // Carrier invocations reflect the merge order: constants then
        // enumerations.
        assert!(
            out.contains("$ crate :: __ankyra_item_constant_CLOCK_FREQ ! ()"),
            "missing constant carrier: {out}"
        );
        assert!(
            out.contains("$ crate :: __ankyra_item_enumeration_MotorKind ! ()"),
            "missing enumeration carrier: {out}"
        );
    }

    fn parse_err(ts: TokenStream2) -> syn::Error {
        match syn::parse2::<ProviderInput>(ts) {
            Ok(_) => panic!("expected parse error"),
            Err(e) => e,
        }
    }

    #[test]
    fn rejects_duplicate_in_list() {
        let err = parse_err(quote! {
            name: P,
            commands: [emergency_stop, emergency_stop],
        });
        assert!(
            err.to_string().contains("duplicate entry `emergency_stop`"),
            "wrong diagnostic: {err}"
        );
    }

    #[test]
    fn rejects_unknown_key() {
        let err = parse_err(quote! {
            name: P,
            widgets: [foo],
        });
        assert!(
            err.to_string()
                .contains("unknown ankyra_provider! key `widgets`"),
            "wrong diagnostic: {err}"
        );
    }

    #[test]
    fn rejects_missing_name() {
        let err = parse_err(quote! {
            commands: [foo],
        });
        assert!(
            err.to_string().contains("requires a `name:` key"),
            "wrong diagnostic: {err}"
        );
    }

    fn expand_reexport_for_test(input: TokenStream2) -> TokenStream2 {
        let parsed: ReexportInput = syn::parse2(input).expect("parse ReexportInput");
        expand_reexport_impl(&parsed)
    }

    #[test]
    fn reexport_emits_two_pub_use_lines() {
        let input = quote! { upstream::CORE_PROVIDER };
        let out = render(&expand_reexport_for_test(input));
        assert!(
            out.contains("pub use upstream :: CORE_PROVIDER"),
            "missing provider re-export: {out}"
        );
        assert!(
            out.contains("pub use upstream :: __ankyra_provider_CORE_PROVIDER"),
            "missing companion re-export: {out}"
        );
    }

    #[test]
    fn reexport_collapses_nested_module_to_crate_root() {
        let input = quote! { upstream::nested::deeper::CORE_PROVIDER };
        let out = render(&expand_reexport_for_test(input));
        // The provider `pub use` preserves the full path.
        assert!(
            out.contains("pub use upstream :: nested :: deeper :: CORE_PROVIDER"),
            "provider re-export truncated: {out}"
        );
        // The companion `pub use` drops intermediate segments per
        // `#[macro_export]` root publication.
        assert!(
            out.contains("pub use upstream :: __ankyra_provider_CORE_PROVIDER"),
            "companion re-export not collapsed: {out}"
        );
    }
}
