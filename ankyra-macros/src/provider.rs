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
//! both `commands` and `replies`, say) are left to the Task 10 assembler;
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
//!    surface is deliberately a placeholder here; the Task 12 assembler
//!    rebuilds the authoritative `message_format` from the command
//!    carrier tuples when building the Klipper data dictionary. The
//!    `ProviderSpec` slices are intended for `ProviderRef::new::<P>()`
//!    consumers that care about counts and item kinds — not wire format.
//! 2. A user-facing `pub const <NAME>: ProviderRef = ProviderRef::new::<...>();`.
//!    This is what crates invoke by referring to `CORE_PROVIDER` in their
//!    own `ankyra_config!` entries.
//! 3. A `#[macro_export] macro_rules! __ankyra_provider_<NAME>` that plays
//!    the role of a continuation in the Task 11 CPS fold. Its body hands
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

use std::collections::HashSet;

use proc_macro::TokenStream;
use proc_macro_error2::abort;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Error, Ident, Path, Token, bracketed, parse_macro_input};

use crate::shared::{carrier_ident, descriptor_ident, provider_companion_ident};

/// Parsed path argument for a `ankyra_provider!` item list entry.
///
/// Accepts two shapes:
/// - A bare ident (`foo`) — item lives at the provider-defining crate's root.
/// - A `crate::…`-prefixed path (`crate::klipper_mod::foo`) — item lives in a
///   submodule of the provider-defining crate.
///
/// Cross-crate paths and `::foo`-style absolute paths are rejected in Task A2
/// so the same-crate / cross-crate split (see `ankyra_reexport_provider!`)
/// stays enforced at one layer.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct ProviderPath {
    path: syn::Path,
}

impl ProviderPath {
    /// Leaf (last) segment's ident — the `#[klipper_*]` item's own name.
    /// This is the protocol-facing name on the wire and the source of the
    /// `#[macro_export]` carrier ident.
    #[allow(dead_code)]
    pub(crate) fn leaf_ident(&self) -> &Ident {
        &self
            .path
            .segments
            .last()
            .expect("ProviderPath invariant: at least one segment")
            .ident
    }

    /// Prefix tokens suitable for splicing into the provider companion
    /// macro's wrapper tuple. `None` for a bare ident (item lives at the
    /// crate root); `Some($crate::a::b)` for a multi-segment path (the
    /// leading `crate` segment is rewritten to `$crate` so the tokens
    /// resolve relative to the provider-defining crate in both same-crate
    /// and cross-crate `ankyra_config!` contexts).
    #[allow(dead_code)]
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

        // Reject generic arguments / turbofish at any segment.
        for seg in &path.segments {
            if !matches!(seg.arguments, syn::PathArguments::None) {
                return Err(syn::Error::new_spanned(
                    seg,
                    "`ankyra_provider!` item paths must be plain paths; generic \
                     arguments / turbofish are not supported",
                ));
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
    commands: Vec<Ident>,
    replies: Vec<Ident>,
    outputs: Vec<Ident>,
    constants: Vec<Ident>,
    enumerations: Vec<Ident>,
}

impl Parse for ProviderInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut name: Option<Ident> = None;
        let mut commands: Option<(proc_macro2::Span, Vec<Ident>)> = None;
        let mut replies: Option<(proc_macro2::Span, Vec<Ident>)> = None;
        let mut outputs: Option<(proc_macro2::Span, Vec<Ident>)> = None;
        let mut constants: Option<(proc_macro2::Span, Vec<Ident>)> = None;
        let mut enumerations: Option<(proc_macro2::Span, Vec<Ident>)> = None;

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
                    let idents = parse_ident_list(input)?;
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

/// Parse a `[ident, ident, ...]` list into a `Vec<Ident>`. Trailing commas
/// and empty lists are both accepted.
fn parse_ident_list(input: ParseStream) -> syn::Result<Vec<Ident>> {
    let body;
    let _brackets = bracketed!(body in input);
    let punct: Punctuated<Ident, Token![,]> = Punctuated::parse_terminated(&body)?;
    Ok(punct.into_iter().collect())
}

/// Enforce per-list uniqueness. Returns the first duplicate's span for a
/// diagnostic that lands on the second occurrence.
fn validate_unique(idents: Vec<Ident>) -> syn::Result<Vec<Ident>> {
    let mut seen: HashSet<String> = HashSet::with_capacity(idents.len());
    for ident in &idents {
        let key = ident.to_string();
        if !seen.insert(key.clone()) {
            return Err(Error::new(
                ident.span(),
                format!("duplicate entry `{key}` in ankyra_provider! list"),
            ));
        }
    }
    Ok(idents)
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

    // MESSAGES: synthesize a command descriptor inline for each command.
    // The message_format placeholder matches the protocol_name. Task 12's
    // assembler builds the authoritative format from the carrier tuples.
    let message_entries: Vec<TokenStream2> = p
        .commands
        .iter()
        .map(|cmd| {
            let name_str = cmd.to_string();
            quote! {
                ::ankyra::descriptor::MessageDescriptor::command(#name_str, #name_str),
            }
        })
        .collect();

    // REPLIES: call the pub const fn emitted by `#[klipper_reply]`.
    let reply_entries: Vec<TokenStream2> = p
        .replies
        .iter()
        .map(|reply| {
            let desc_fn = descriptor_ident(reply);
            quote! { #desc_fn(), }
        })
        .collect();

    // OUTPUTS: call the pub const fn emitted by `#[klipper_output]`.
    let output_entries: Vec<TokenStream2> = p
        .outputs
        .iter()
        .map(|out| {
            let desc_fn = descriptor_ident(out);
            quote! { #desc_fn(), }
        })
        .collect();

    // DEFINITIONS: both constants and enumerations contribute here. The
    // descriptor fn shape is identical across `#[klipper_constant]` and
    // `klipper_enumeration!`, so we can concatenate both lists.
    let definition_entries: Vec<TokenStream2> = p
        .constants
        .iter()
        .chain(p.enumerations.iter())
        .map(|d| {
            let desc_fn = descriptor_ident(d);
            quote! { #desc_fn(), }
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

/// Emit one carrier-invocation line for the companion macro body:
/// `$crate::__ankyra_item_<kind>_<ident>!(),`.
fn carrier_call(kind: &str, ident: &Ident) -> TokenStream2 {
    let carrier = carrier_ident(kind, ident);
    quote! { $crate::#carrier!(), }
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
            msg.contains("generic arguments") || msg.contains("turbofish"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn rejects_generic_on_bare_ident() {
        let err = syn::parse_str::<ProviderPath>("Foo<T>").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("generic arguments") || msg.contains("turbofish"),
            "unexpected error: {msg}"
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
