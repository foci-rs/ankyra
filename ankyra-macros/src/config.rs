//! `ankyra_config!` function-like proc-macro.
//!
//! # Shape
//!
//! ```ignore
//! ankyra_config! {
//!     transport = crate::TRANSPORT_OUTPUT: crate::BufferTransportOutput,
//!     context = &'ctx mut ClockContext,
//!     providers = [crate::CORE_PROVIDER, clock_lib::CLOCK_PROVIDER],
//!     static_strings = ["test probe", "fan stuck"],
//!     app = "clock-firmware",
//!     version = env!("CARGO_PKG_VERSION"),
//!     build_versions = "",
//!     license = "MIT OR Apache-2.0",
//! }
//! ```
//!
//! `transport`, `context`, and `providers` are required; everything else is
//! optional. Any of the keys may appear in any order. `static_strings`
//! defaults to the empty list — a firmware that never references a
//! `klipper_static_string!("...")` literal or `klipper_shutdown!("...", ...)`
//! reason string does not need to list anything. `transport` uses `path: ty`
//! colon syntax so the value and its type land in the same key without
//! forcing two separate keys.
//!
//! `app`, `version`, `build_versions`, and `license` customize the trailer
//! fields of the Klipper data dictionary. Each takes any expression that
//! evaluates to a `&'static str` (a plain string literal, `env!(...)`,
//! `concat!(...)`, or a module-path constant). Omitted keys fall back to
//! ankyra's own defaults (`"ankyra"`, `"ankyra-v0.1"`, `concat!("ankyra-",
//! env!("CARGO_PKG_VERSION"))` evaluated inside `ankyra-assemble`, and
//! `"MIT OR Apache-2.0"` respectively) so the wire format is unchanged for
//! consumers that do not opt in.
//!
//! # Expansion
//!
//! `ankyra_config!` does not itself produce the final module tree. It is a
//! thin shim that launches a continuation-passing fold:
//!
//! ```ignore
//! ::ankyra::__ankyra_fold_providers! {
//!     config = {
//!         transport_path = <path>,
//!         transport_ty   = <ty>,
//!         context_ty     = <ty>,
//!         static_strings = [ "test probe", "fan stuck" ],
//!         app            = <expr>,
//!         version        = <expr>,
//!         build_versions = <expr>,
//!         license        = <expr>,
//!     },
//!     accumulator = [],
//!     remaining = [ crate::__ankyra_provider_CORE_PROVIDER, clock_lib::__ankyra_provider_CLOCK_PROVIDER ],
//! }
//! ```
//!
//! Each of the four metadata keys is optional. When the user omits one, the
//! corresponding sub-key in the fold is simply not emitted; `ankyra-assemble`
//! substitutes its own default at dictionary-emit time.
//!
//! Each companion macro in `remaining` appends its items' carrier tuples to
//! `accumulator` and tail-calls back into `__ankyra_fold_providers!` with
//! its own entry stripped from `remaining`. When `remaining` is empty the
//! fold delegates to `__ankyra_assemble!` (in `ankyra-assemble`) which
//! synthesizes the dispatch table, sender impls, data dictionary, and
//! `KLIPPER_TRANSPORT` binding.
//!
//! The path rewrite `crate::CORE_PROVIDER` →
//! `crate::__ankyra_provider_CORE_PROVIDER` is delegated to
//! [`crate::shared::provider_path_to_companion`]; that helper rejects bare
//! idents with a span-pointed diagnostic because `#[macro_export]`
//! publishes companion macros at the defining crate's root, not the module
//! the `ankyra_provider!` invocation lived in.

use proc_macro::TokenStream;
use proc_macro_error2::abort;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Error, Expr, Ident, LitStr, Path, Token, Type, bracketed, parse_macro_input};

/// Parsed shape of `ankyra_config! { key = value, ... }`.
pub struct AnkyraConfigInput {
    pub transport_path: Path,
    pub transport_ty: Type,
    pub context_ty: Type,
    pub providers: Vec<Path>,
    /// Every literal referenced by `klipper_static_string!` /
    /// `klipper_shutdown!` call sites in the firmware. Optional at parse
    /// time — a missing key becomes an empty Vec.
    pub static_strings: Vec<LitStr>,
    /// Override for the data dictionary's `"app"` trailer field. Any
    /// `&'static str`-valued expression (literal, `env!(...)`,
    /// `concat!(...)`, module constant). `None` falls through to ankyra's
    /// `"ankyra"` default at the assembler side.
    pub app: Option<Expr>,
    /// Override for the dictionary's `"version"` field. `None` ⇒
    /// `"ankyra-v0.1"`.
    pub version: Option<Expr>,
    /// Override for the dictionary's `"build_versions"` field. `None` ⇒
    /// `concat!("ankyra-", env!("CARGO_PKG_VERSION"))` evaluated at
    /// `ankyra-assemble`'s compile site (so the value is ankyra's own
    /// package version, not the consuming crate's).
    pub build_versions: Option<Expr>,
    /// Override for the dictionary's `"license"` field. `None` ⇒
    /// `"MIT OR Apache-2.0"`.
    pub license: Option<Expr>,
}

impl Parse for AnkyraConfigInput {
    // Eight supported keys times one small arm each plus the
    // validate-required-keys tail pushes this past clippy's default
    // 100-line ceiling. Splitting each arm into its own helper would
    // not make the parser clearer — every arm is a simple
    // "already-set? error : parse-one-value" pattern — so the allow is
    // more honest than a mechanical factoring.
    #[allow(clippy::too_many_lines)]
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut transport: Option<(Path, Type)> = None;
        let mut context_ty: Option<Type> = None;
        let mut providers: Option<Vec<Path>> = None;
        let mut static_strings: Option<Vec<LitStr>> = None;
        let mut app: Option<Expr> = None;
        let mut version: Option<Expr> = None;
        let mut build_versions: Option<Expr> = None;
        let mut license: Option<Expr> = None;

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            let _: Token![=] = input.parse()?;
            match key.to_string().as_str() {
                "transport" => {
                    if transport.is_some() {
                        return Err(Error::new(key.span(), "duplicate `transport` key"));
                    }
                    let path: Path = input.parse()?;
                    let _: Token![:] = input.parse()?;
                    let ty: Type = input.parse()?;
                    transport = Some((path, ty));
                }
                "context" => {
                    if context_ty.is_some() {
                        return Err(Error::new(key.span(), "duplicate `context` key"));
                    }
                    let ty: Type = input.parse()?;
                    context_ty = Some(ty);
                }
                "providers" => {
                    if providers.is_some() {
                        return Err(Error::new(key.span(), "duplicate `providers` key"));
                    }
                    let list;
                    bracketed!(list in input);
                    let items: Punctuated<Path, Token![,]> = Punctuated::parse_terminated(&list)?;
                    providers = Some(items.into_iter().collect());
                }
                "static_strings" => {
                    if static_strings.is_some() {
                        return Err(Error::new(key.span(), "duplicate `static_strings` key"));
                    }
                    let list;
                    bracketed!(list in input);
                    let items: Punctuated<LitStr, Token![,]> = Punctuated::parse_terminated(&list)?;
                    static_strings = Some(items.into_iter().collect());
                }
                "app" => {
                    if app.is_some() {
                        return Err(Error::new(key.span(), "duplicate `app` key"));
                    }
                    app = Some(input.parse::<Expr>()?);
                }
                "version" => {
                    if version.is_some() {
                        return Err(Error::new(key.span(), "duplicate `version` key"));
                    }
                    version = Some(input.parse::<Expr>()?);
                }
                "build_versions" => {
                    if build_versions.is_some() {
                        return Err(Error::new(key.span(), "duplicate `build_versions` key"));
                    }
                    build_versions = Some(input.parse::<Expr>()?);
                }
                "license" => {
                    if license.is_some() {
                        return Err(Error::new(key.span(), "duplicate `license` key"));
                    }
                    license = Some(input.parse::<Expr>()?);
                }
                other => {
                    return Err(Error::new(
                        key.span(),
                        format!(
                            "unknown ankyra_config! key `{other}`; expected one of: \
                             transport, context, providers, static_strings, \
                             app, version, build_versions, license"
                        ),
                    ));
                }
            }
            if input.peek(Token![,]) {
                let _: Token![,] = input.parse()?;
            } else {
                break;
            }
        }

        let Some((transport_path, transport_ty)) = transport else {
            return Err(Error::new(
                proc_macro2::Span::call_site(),
                "ankyra_config! requires a `transport = <path>: <type>` entry",
            ));
        };
        let Some(context_ty) = context_ty else {
            return Err(Error::new(
                proc_macro2::Span::call_site(),
                "ankyra_config! requires a `context = <type>` entry",
            ));
        };
        let Some(providers) = providers else {
            return Err(Error::new(
                proc_macro2::Span::call_site(),
                "ankyra_config! requires a `providers = [<path>, ...]` entry",
            ));
        };
        let static_strings = static_strings.unwrap_or_default();

        Ok(Self {
            transport_path,
            transport_ty,
            context_ty,
            providers,
            static_strings,
            app,
            version,
            build_versions,
            license,
        })
    }
}

/// Entry point for `ankyra_config! { ... }` expansion.
pub fn expand_ankyra_config(input: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(input as AnkyraConfigInput);
    expand_ankyra_config_impl(&parsed).into()
}

fn expand_ankyra_config_impl(input: &AnkyraConfigInput) -> TokenStream2 {
    let companions: Vec<Path> = input
        .providers
        .iter()
        .map(|p| match crate::shared::provider_path_to_companion(p) {
            Ok(path) => path,
            Err(e) => abort!(e.span(), "{}", e),
        })
        .collect();

    let transport_path = &input.transport_path;
    let transport_ty = &input.transport_ty;
    let context_ty = &input.context_ty;
    let static_strings = &input.static_strings;

    // Each metadata key is only emitted when the user actually supplied
    // one. A missing key becomes an absent sub-field in the fold token
    // stream, which `ankyra-assemble` reads as "apply the default".
    let app_kv = input
        .app
        .as_ref()
        .map(|e| quote!(app = #e,))
        .unwrap_or_default();
    let version_kv = input
        .version
        .as_ref()
        .map(|e| quote!(version = #e,))
        .unwrap_or_default();
    let build_versions_kv = input
        .build_versions
        .as_ref()
        .map(|e| quote!(build_versions = #e,))
        .unwrap_or_default();
    let license_kv = input
        .license
        .as_ref()
        .map(|e| quote!(license = #e,))
        .unwrap_or_default();

    quote! {
        ::ankyra::__ankyra_fold_providers! {
            config = {
                transport_path = #transport_path,
                transport_ty = #transport_ty,
                context_ty = #context_ty,
                static_strings = [ #(#static_strings),* ],
                #app_kv
                #version_kv
                #build_versions_kv
                #license_kv
            },
            accumulator = [],
            remaining = [ #(#companions),* ],
        }
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
        let parsed: AnkyraConfigInput = syn::parse2(input).expect("parse AnkyraConfigInput");
        expand_ankyra_config_impl(&parsed)
    }

    #[test]
    fn expands_minimal_config() {
        let input = quote! {
            transport = crate::TRANSPORT_OUTPUT: crate::BufferTransportOutput,
            context = &'ctx mut (),
            providers = [crate::CORE_PROVIDER],
        };
        let out = render(&expand_for_test(input));
        assert!(
            out.contains(":: ankyra :: __ankyra_fold_providers !"),
            "missing fold entry: {out}"
        );
        assert!(
            out.contains("transport_path = crate :: TRANSPORT_OUTPUT"),
            "missing transport path: {out}"
        );
        assert!(
            out.contains("transport_ty = crate :: BufferTransportOutput"),
            "missing transport ty: {out}"
        );
        assert!(
            out.contains("context_ty = & 'ctx mut ()"),
            "missing context_ty: {out}"
        );
        assert!(
            out.contains("remaining = [__ankyra_provider_CORE_PROVIDER]"),
            "missing companion in remaining: {out}"
        );
        assert!(
            out.contains("static_strings = []"),
            "missing empty static_strings: {out}"
        );
    }

    #[test]
    fn static_strings_are_threaded_through() {
        let input = quote! {
            transport = crate::T: crate::Ty,
            context = &mut (),
            providers = [crate::P],
            static_strings = ["alpha", "beta"],
        };
        let out = render(&expand_for_test(input));
        assert!(
            out.contains(r#"static_strings = ["alpha" , "beta"]"#),
            "static_strings not preserved: {out}"
        );
    }

    #[test]
    fn accepts_keys_in_any_order() {
        let input = quote! {
            static_strings = ["x"],
            providers = [crate::P],
            context = &mut (),
            transport = crate::T: crate::Ty,
        };
        let out = render(&expand_for_test(input));
        assert!(out.contains("remaining = [__ankyra_provider_P]"));
        assert!(out.contains(r#"static_strings = ["x"]"#));
    }

    fn parse_err(ts: TokenStream2) -> syn::Error {
        match syn::parse2::<AnkyraConfigInput>(ts) {
            Ok(_) => panic!("expected parse error"),
            Err(e) => e,
        }
    }

    #[test]
    fn rejects_missing_transport() {
        let err = parse_err(quote! {
            context = &mut (),
            providers = [crate::P],
        });
        assert!(
            err.to_string().contains("requires a `transport"),
            "wrong diagnostic: {err}"
        );
    }

    #[test]
    fn rejects_missing_context() {
        let err = parse_err(quote! {
            transport = crate::T: crate::Ty,
            providers = [crate::P],
        });
        assert!(
            err.to_string().contains("requires a `context"),
            "wrong diagnostic: {err}"
        );
    }

    #[test]
    fn rejects_missing_providers() {
        let err = parse_err(quote! {
            transport = crate::T: crate::Ty,
            context = &mut (),
        });
        assert!(
            err.to_string().contains("requires a `providers"),
            "wrong diagnostic: {err}"
        );
    }

    #[test]
    fn rejects_unknown_key() {
        let err = parse_err(quote! {
            transport = crate::T: crate::Ty,
            context = &mut (),
            providers = [crate::P],
            widgets = [foo],
        });
        assert!(
            err.to_string()
                .contains("unknown ankyra_config! key `widgets`"),
            "wrong diagnostic: {err}"
        );
    }

    #[test]
    fn rejects_duplicate_transport() {
        let err = parse_err(quote! {
            transport = crate::T: crate::Ty,
            transport = crate::T2: crate::Ty2,
            context = &mut (),
            providers = [crate::P],
        });
        assert!(
            err.to_string().contains("duplicate `transport`"),
            "wrong diagnostic: {err}"
        );
    }

    #[test]
    fn metadata_keys_thread_into_fold_body() {
        // Every metadata key the user supplies should appear verbatim in
        // the fold's `config = { ... }` block so `ankyra-assemble` can read
        // it back. Omitted keys must not produce stray sub-fields.
        let input = quote! {
            transport = crate::T: crate::Ty,
            context = &mut (),
            providers = [crate::P],
            app = "foci",
            version = env!("CARGO_PKG_VERSION"),
            build_versions = "",
            license = "MIT OR Apache-2.0",
        };
        let out = render(&expand_for_test(input));
        let normalized = out.replace(' ', "");
        assert!(
            normalized.contains(r#"app="foci""#),
            "app override not threaded: {out}"
        );
        assert!(
            normalized.contains(r#"version=env!("CARGO_PKG_VERSION")"#),
            "version override not threaded: {out}"
        );
        assert!(
            normalized.contains(r#"build_versions="""#),
            "build_versions override not threaded: {out}"
        );
        assert!(
            normalized.contains(r#"license="MITORApache-2.0""#),
            "license override not threaded: {out}"
        );
    }

    #[test]
    fn metadata_keys_are_optional() {
        // Omitting all four metadata keys must not produce any `app =`,
        // `version =`, `build_versions =`, or `license =` sub-field in the
        // fold body — the assembler side distinguishes "absent" from
        // "supplied" to pick defaults.
        let input = quote! {
            transport = crate::T: crate::Ty,
            context = &mut (),
            providers = [crate::P],
        };
        let out = render(&expand_for_test(input));
        for key in ["app = ", "version = ", "build_versions = ", "license = "] {
            assert!(
                !out.contains(key),
                "omitted metadata key `{key}` leaked into fold body: {out}"
            );
        }
    }

    #[test]
    fn rejects_duplicate_app() {
        let err = parse_err(quote! {
            transport = crate::T: crate::Ty,
            context = &mut (),
            providers = [crate::P],
            app = "foo",
            app = "bar",
        });
        assert!(
            err.to_string().contains("duplicate `app`"),
            "wrong diagnostic: {err}"
        );
    }

    #[test]
    fn unknown_key_diagnostic_lists_metadata_keys() {
        // When a typo lands on an unknown key the error message should
        // enumerate every accepted key (including the four metadata
        // overrides) so the user can spot the intended one.
        let err = parse_err(quote! {
            transport = crate::T: crate::Ty,
            context = &mut (),
            providers = [crate::P],
            apps = "oops",
        });
        let msg = err.to_string();
        assert!(msg.contains("`apps`"), "wrong diagnostic: {msg}");
        for key in ["app", "version", "build_versions", "license"] {
            assert!(msg.contains(key), "diagnostic must list `{key}`: {msg}");
        }
    }
}
