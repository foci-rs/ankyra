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
//! 1. A hidden `const _` block that calls every listed reply, output,
//!    constant, and enumeration's `__ankyra_descriptor_<T>()`, so a
//!    provider crate that names a missing item fails in its own build.
//! 2. A user-facing `pub const <NAME>: ProviderRef`, the handle crates name
//!    in their own `ankyra_config!` entries.
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
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Error, Ident, Path, Token, bracketed, parse_macro_input};

use crate::shared::{carrier_ident_with_lifetimes, descriptor_ident, provider_companion_ident};

#[derive(Debug, Clone)]
pub(crate) struct ProviderPath {
    path: syn::Path,
}

impl ProviderPath {
    pub(crate) fn leaf_ident(&self) -> &Ident {
        &self
            .path
            .segments
            .last()
            .expect("ProviderPath invariant: at least one segment")
            .ident
    }

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

    pub(crate) fn prefix_rooted_at(&self, crate_root: &TokenStream2) -> Option<TokenStream2> {
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
                    crate_root.clone()
                } else {
                    quote::quote!(#ident)
                }
            })
            .collect();
        Some(quote::quote! { #(#rewritten)::* })
    }

    pub(crate) fn as_path(&self) -> &syn::Path {
        &self.path
    }
}

impl syn::parse::Parse for ProviderPath {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let path: syn::Path = input.parse()?;

        if path.leading_colon.is_some() {
            return Err(syn::Error::new_spanned(
                &path,
                "`ankyra_provider!` item paths must start with `crate::` or be a \
                 bare ident; absolute paths with leading `::` are not supported. \
                 Cross-crate items must go through `ankyra_reexport_provider!`.",
            ));
        }

        if path.segments.len() > 1 && path.segments[0].ident != "crate" {
            return Err(syn::Error::new_spanned(
                &path,
                "`ankyra_provider!` item paths must start with `crate::` or be a \
                 bare ident; cross-crate items must go through \
                 `ankyra_reexport_provider!`.",
            ));
        }

        let last_idx = path.segments.len() - 1;
        for seg in path.segments.iter().take(last_idx) {
            if !matches!(seg.arguments, syn::PathArguments::None) {
                return Err(syn::Error::new_spanned(
                    seg,
                    "`ankyra_provider!` item paths must be plain paths; generic \
                     arguments on non-leaf segments are not supported",
                ));
            }
        }
        let leaf = &path.segments[last_idx];
        match &leaf.arguments {
            syn::PathArguments::None => {}
            syn::PathArguments::AngleBracketed(ab) => {
                if let Some(other) = ab
                    .args
                    .iter()
                    .find(|arg| !matches!(arg, syn::GenericArgument::Lifetime(_)))
                {
                    return Err(syn::Error::new_spanned(
                        other,
                        "`ankyra_provider!` reply/output leaf paths \
                         may only carry lifetime arguments; type \
                         and const generics are not supported",
                    ));
                }
            }
            syn::PathArguments::Parenthesized(_) => {
                return Err(syn::Error::new_spanned(
                    leaf,
                    "`ankyra_provider!` item paths do not accept Fn-style \
                     parenthesized generic arguments",
                ));
            }
        }

        Ok(Self { path })
    }
}

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
        let mut commands: Option<Vec<ProviderPath>> = None;
        let mut replies: Option<Vec<ProviderPath>> = None;
        let mut outputs: Option<Vec<ProviderPath>> = None;
        let mut constants: Option<Vec<ProviderPath>> = None;
        let mut enumerations: Option<Vec<ProviderPath>> = None;

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
                    *slot = Some(idents);
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

        let commands = validate_unique(commands.unwrap_or_default())?;
        let replies = validate_unique(replies.unwrap_or_default())?;
        let outputs = validate_unique(outputs.unwrap_or_default())?;
        let constants = validate_unique(constants.unwrap_or_default())?;
        let enumerations = validate_unique(enumerations.unwrap_or_default())?;

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

fn parse_provider_path_list(input: ParseStream) -> syn::Result<Vec<ProviderPath>> {
    let body;
    let _brackets = bracketed!(body in input);
    let punct: Punctuated<ProviderPath, Token![,]> = Punctuated::parse_terminated(&body)?;
    Ok(punct.into_iter().collect())
}

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

pub fn expand_provider(input: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(input as ProviderInput);
    expand_provider_impl(&parsed).into()
}

fn expand_provider_impl(p: &ProviderInput) -> TokenStream2 {
    let name = &p.name;
    let companion_ident = provider_companion_ident(name);

    let typed = |entries: &[ProviderPath], ty: TokenStream2| -> Vec<TokenStream2> {
        entries
            .iter()
            .map(|e| {
                let call = qualify_descriptor(e);
                quote! { let _: ::ankyra::descriptor::#ty = #call; }
            })
            .collect()
    };
    let item_checks: Vec<TokenStream2> = typed(&p.replies, quote!(ReplyDescriptor))
        .into_iter()
        .chain(typed(&p.outputs, quote!(OutputDescriptor)))
        .chain(typed(&p.constants, quote!(DefinitionDescriptor)))
        .chain(typed(&p.enumerations, quote!(DefinitionDescriptor)))
        .collect();
    let item_check = quote! {
        const _: () = {
            #(#item_checks)*
        };
    };

    let user_const = quote! {
        pub const #name: ::ankyra::provider::ProviderRef =
            ::ankyra::provider::ProviderRef::__new();
    };

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
        #item_check
        #user_const
        #companion_macro
    }
}

fn qualify_descriptor(entry: &ProviderPath) -> TokenStream2 {
    let leaf = entry.leaf_ident();
    let desc = descriptor_ident(leaf);
    if let Some(prefix) = entry.prefix_rooted_at(&quote!(crate)) {
        quote! { #prefix::#desc() }
    } else {
        quote! { crate::#desc() }
    }
}

fn carrier_call(kind: &str, entry: &ProviderPath) -> TokenStream2 {
    let lifetime_count = match kind {
        "reply" | "output" => entry.leaf_lifetime_count(),
        _ => 0,
    };
    let carrier = carrier_ident_with_lifetimes(kind, entry.leaf_ident(), lifetime_count);
    let prefix_inner = entry
        .prefix_rooted_at(&quote!($crate))
        .unwrap_or_else(|| quote!($crate));
    quote! {
        { prefix: (#prefix_inner), $crate::#carrier!() },
    }
}

struct ReexportInput {
    path: Path,
}

impl Parse for ReexportInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let path: Path = input.parse()?;
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
        assert!(
            p.prefix_rooted_at(&quote::quote!($crate)).is_none(),
            "bare ident has no prefix"
        );
    }

    #[test]
    fn parses_crate_path() {
        let p: ProviderPath = parse_quote!(crate::klipper_mod::get_clock);
        assert_eq!(p.leaf_ident().to_string(), "get_clock");
        let prefix = p
            .prefix_rooted_at(&quote::quote!($crate))
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
        let p: ProviderPath = parse_quote!(Pong);
        let out = super::qualify_descriptor(&p).to_string();
        assert_eq!(out.replace(' ', ""), "crate::__ankyra_descriptor_Pong()");
    }

    #[test]
    fn qualify_descriptor_for_path() {
        let p: ProviderPath = parse_quote!(crate::submod::Pong);
        let out = super::qualify_descriptor(&p).to_string();
        assert_eq!(
            out.replace(' ', ""),
            "crate::submod::__ankyra_descriptor_Pong()"
        );
    }

    #[test]
    fn validate_unique_rejects_same_leaf_across_paths() {
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
        let a: ProviderPath = parse_quote!(crate::a::foo);
        let b: ProviderPath = parse_quote!(crate::b::bar);
        super::validate_unique(vec![a, b]).expect("distinct leaves pass");
    }

    #[test]
    fn validate_unique_rejects_duplicate_identical_paths() {
        let a: ProviderPath = parse_quote!(crate::a::foo);
        let b: ProviderPath = parse_quote!(crate::a::foo);
        let err = super::validate_unique(vec![a, b]).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("duplicate entry `foo`"),
            "missing duplicate prefix: {msg}"
        );
        assert!(
            !msg.contains("(first:"),
            "identical-path dedup should omit the parenthetical: {msg}"
        );
    }

    #[test]
    fn provider_input_parses_mixed_entries() {
        let input: super::ProviderInput = parse_quote! {
            name: P,
            commands: [foo, crate::sub::bar],
        };
        assert_eq!(input.commands.len(), 2);
        assert_eq!(input.commands[0].leaf_ident().to_string(), "foo");
        assert_eq!(input.commands[1].leaf_ident().to_string(), "bar");
        assert!(
            input.commands[0]
                .prefix_rooted_at(&quote::quote!($crate))
                .is_none()
        );
        let bar_prefix = input.commands[1]
            .prefix_rooted_at(&quote::quote!($crate))
            .unwrap();
        assert_eq!(bar_prefix.to_string().replace(' ', ""), "$crate::sub");
    }

    #[test]
    fn carrier_call_wraps_with_dollar_crate_prefix_for_bare_ident() {
        let entry: ProviderPath = parse_quote!(emergency_stop);
        let out = super::carrier_call("command", &entry).to_string();
        let normalised = out.replace(' ', "");
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
    fn minimal_provider_emits_item_checks_handle_and_companion() {
        let input = quote! {
            name: CORE_PROVIDER,
            commands: [emergency_stop],
            replies: [PingReply],
        };
        let out = render(&expand_for_test(input));
        assert!(
            out.contains("pub const CORE_PROVIDER : :: ankyra :: provider :: ProviderRef"),
            "missing user const: {out}"
        );
        assert!(
            out.contains("__ankyra_descriptor_PingReply ()"),
            "missing reply descriptor call: {out}"
        );
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
    fn definitions_merge_constants_and_enumerations() {
        let input = quote! {
            name: DICT,
            constants: [CLOCK_FREQ],
            enumerations: [MotorKind],
        };
        let out = render(&expand_for_test(input));
        assert!(
            out.contains("__ankyra_descriptor_CLOCK_FREQ ()"),
            "missing constant desc: {out}"
        );
        assert!(
            out.contains("__ankyra_descriptor_MotorKind ()"),
            "missing enumeration desc: {out}"
        );
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
        assert!(
            out.contains("pub use upstream :: nested :: deeper :: CORE_PROVIDER"),
            "provider re-export truncated: {out}"
        );
        assert!(
            out.contains("pub use upstream :: __ankyra_provider_CORE_PROVIDER"),
            "companion re-export not collapsed: {out}"
        );
    }
}
