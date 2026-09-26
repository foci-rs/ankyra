//! `klipper_static_string!`, `klipper_shutdown!`, and
//! `klipper_shutdown_from!` function-like macros.
//!
//! # `klipper_static_string!("msg")`
//!
//! Expands to `crate::_ankyra_config::static_strings::__ANKYRA_SS_<hash>`.
//! The hash is the FNV-1a 64-bit digest of the literal's UTF-8 bytes as
//! computed by [`crate::shared::static_string_hash_ident`]; the same function
//! is used by the assembler so both sides agree on the symbol name.
//!
//! The emitted path is plain `crate::…` (not `$crate::…`) because proc-macros
//! emit literal tokens that resolve against the call site's crate, which is
//! exactly the crate that runs `ankyra_config!` to declare the
//! `_ankyra_config::static_strings` module. If the literal is not listed in
//! that module's declarative `static_strings = [...]` entry, the firmware
//! build fails with `cannot find __ANKYRA_SS_<hash> in module static_strings`
//! — a clear diagnostic distinct from "static string silently mis-registered".
//!
//! No carrier macro is emitted here: unlike replies/outputs/commands,
//! static strings never cross the provider boundary. They are registered
//! firmware-locally through `ankyra_config!`.
//!
//! # `klipper_shutdown!("msg", clock_expr)`
//!
//! Expands to
//!
//! ```ignore
//! <_ as ::ankyra::SendReply<::ankyra::Shutdown>>::send(
//!     __ankyra_sender,
//!     ::ankyra::Shutdown {
//!         clock: <clock_expr>,
//!         static_string_id: crate::_ankyra_config::static_strings::__ANKYRA_SS_<hash>,
//!     },
//! )
//! ```
//!
//! Must be invoked from inside a `#[klipper_command]` handler body because
//! the emitted code references `__ankyra_sender`, a parameter injected by
//! that attribute's dispatch wrapper. The handler body-scan recognises
//! `klipper_shutdown!` invocations and folds an `S: SendReply<Shutdown>`
//! bound onto the wrapper's generic.
//!
//! # `klipper_shutdown_from!(sender_expr, "msg", clock_expr)`
//!
//! Non-handler-context variant of `klipper_shutdown!`. The caller supplies
//! the sender explicitly as the first argument; the static-string handling
//! is identical to `klipper_shutdown!`. Expands to
//!
//! ```ignore
//! {
//!     let __ankyra_sender = <sender_expr>;
//!     <_ as ::ankyra::SendReply<::ankyra::Shutdown>>::send(
//!         __ankyra_sender,
//!         ::ankyra::Shutdown {
//!             clock: <clock_expr>,
//!             static_string_id: crate::_ankyra_config::static_strings::__ANKYRA_SS_<hash>,
//!         },
//!     )
//! }
//! ```
//!
//! The sender expression is bound to a local first so it is evaluated exactly
//! once, mirroring [`crate::reply::expand_reply_from_call_site`] and
//! [`crate::output::expand_output_from_call_site`].

use proc_macro::TokenStream;
use proc_macro_error2::{abort, abort_call_site};
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::parse::{Parse, ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::{Expr, ExprLit, Lit, LitStr, Token, parse_macro_input};

use crate::shared::static_string_hash_ident;

/// Entry point for `klipper_static_string!("msg")`.
pub fn expand_static_string(tokens: TokenStream) -> TokenStream {
    let lit: LitStr = match syn::parse::<LitStr>(tokens) {
        Ok(l) => l,
        Err(e) => abort!(e.span(), "{}", e),
    };
    let hash_ident = static_string_hash_ident(&lit.value(), lit.span());
    // Emit plain `crate::…`, not `$crate::…`. See module docs.
    let out: TokenStream2 = quote! {
        crate::_ankyra_config::static_strings::#hash_ident
    };
    out.into()
}

/// Entry point for `klipper_shutdown!("msg", clock_expr)`.
pub fn expand_shutdown(tokens: TokenStream) -> TokenStream {
    let parser = Punctuated::<Expr, Token![,]>::parse_terminated;
    let args = match parser.parse(tokens) {
        Ok(a) => a,
        Err(e) => abort!(e.span(), "{}", e),
    };

    let mut iter = args.into_iter();
    let Some(msg_expr) = iter.next() else {
        abort_call_site!(
            "klipper_shutdown! requires two arguments: a string literal message and a clock expression"
        );
    };
    let Some(clock_expr) = iter.next() else {
        // No clock expression; point at the message expr end since there is
        // no "missing" token to span.
        abort!(
            msg_expr,
            "klipper_shutdown! requires a clock expression as its second argument"
        );
    };
    if let Some(extra) = iter.next() {
        // Point the diagnostic at the offending extra argument rather than
        // the whole invocation.
        abort!(
            extra,
            "klipper_shutdown! accepts exactly two arguments: a string literal message and a clock expression"
        );
    }

    let lit: LitStr = match msg_expr {
        Expr::Lit(ExprLit {
            lit: Lit::Str(s), ..
        }) => s,
        other => abort!(
            other,
            "klipper_shutdown! first argument must be a string literal"
        ),
    };

    let hash_ident = static_string_hash_ident(&lit.value(), lit.span());
    let out: TokenStream2 = quote! {
        <_ as ::ankyra::SendReply<::ankyra::Shutdown>>::send(
            __ankyra_sender,
            ::ankyra::Shutdown {
                clock: #clock_expr,
                static_string_id: crate::_ankyra_config::static_strings::#hash_ident,
            },
        )
    };
    out.into()
}

/// Parsed `klipper_shutdown_from!(sender_expr, "msg", clock_expr)`.
///
/// Mirrors `crate::reply::ReplyFromCallSite` / the equivalent output
/// parser: the sender expression must appear first, separated by a comma
/// from the remaining handler-scoped shape (`"literal", clock_expr`). The
/// parser emits a helpful usage diagnostic when the sender is missing or
/// the separating comma is absent.
struct ShutdownFromCallSite {
    sender_expr: Expr,
    reason: LitStr,
    clock_expr: Expr,
}

impl Parse for ShutdownFromCallSite {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let sender_expr: Expr = input.parse()?;
        let _comma: Token![,] = input.parse().map_err(|_| {
            syn::Error::new(
                input.span(),
                "klipper_shutdown_from! requires a sender expression followed by a comma, \
                 then a string literal reason, then a clock expression: \
                 klipper_shutdown_from!(sender_expr, \"reason\", clock_expr)",
            )
        })?;
        let reason: LitStr = input.parse().map_err(|_| {
            syn::Error::new(
                input.span(),
                "klipper_shutdown_from! second argument must be a string literal reason",
            )
        })?;
        let _comma: Token![,] = input.parse().map_err(|_| {
            syn::Error::new(
                input.span(),
                "klipper_shutdown_from! requires a clock expression after the reason literal",
            )
        })?;
        let clock_expr: Expr = input.parse()?;
        // Disallow trailing tokens so stray arguments surface a clear error
        // rather than silently being ignored.
        if !input.is_empty() {
            // Accept an optional trailing comma for ergonomics, but nothing else.
            let _trailing: Token![,] = input.parse().map_err(|_| {
                syn::Error::new(
                    input.span(),
                    "klipper_shutdown_from! accepts exactly three arguments: \
                     sender_expr, reason literal, clock expression",
                )
            })?;
            if !input.is_empty() {
                return Err(syn::Error::new(
                    input.span(),
                    "klipper_shutdown_from! accepts exactly three arguments: \
                     sender_expr, reason literal, clock expression",
                ));
            }
        }
        Ok(Self {
            sender_expr,
            reason,
            clock_expr,
        })
    }
}

/// Entry point for `klipper_shutdown_from!(sender_expr, "msg", clock_expr)`.
pub fn expand_shutdown_from_call_site(input: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(input as ShutdownFromCallSite);
    expand_shutdown_from_call_site_impl(&parsed).into()
}

fn expand_shutdown_from_call_site_impl(call: &ShutdownFromCallSite) -> TokenStream2 {
    let sender = &call.sender_expr;
    let clock = &call.clock_expr;
    let hash_ident = static_string_hash_ident(&call.reason.value(), call.reason.span());
    // Bind the sender to a local first so any side-effectful sender
    // expression is evaluated exactly once — matches `klipper_reply_from!`
    // and `klipper_output_from!`. The binding name reuses `__ankyra_sender`
    // so the emitted dispatch call is byte-identical to the handler-scoped
    // `klipper_shutdown!` expansion.
    quote! {
        {
            let __ankyra_sender = #sender;
            <_ as ::ankyra::SendReply<::ankyra::Shutdown>>::send(
                __ankyra_sender,
                ::ankyra::Shutdown {
                    clock: #clock,
                    static_string_id: crate::_ankyra_config::static_strings::#hash_ident,
                },
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proc_macro2::TokenStream as TokenStream2;
    use quote::quote;

    fn expand_ss_for_test(input: TokenStream2) -> TokenStream2 {
        let lit: LitStr = syn::parse2(input).expect("parse LitStr");
        let hash_ident = static_string_hash_ident(&lit.value(), lit.span());
        quote! {
            crate::_ankyra_config::static_strings::#hash_ident
        }
    }

    #[test]
    fn static_string_expands_to_config_path() {
        let out = expand_ss_for_test(quote!("probe")).to_string();
        // Hash `probe` matches the pinned fixture in shared::fnv_tests.
        assert!(
            out.contains("__ANKYRA_SS_f97691246db266f1"),
            "wrong hash ident in path: {out}"
        );
        assert!(
            out.contains("crate :: _ankyra_config :: static_strings"),
            "expected static_strings module path: {out}"
        );
    }

    fn expand_shutdown_for_test(input: TokenStream2) -> TokenStream2 {
        let parser = Punctuated::<Expr, Token![,]>::parse_terminated;
        let args = parser
            .parse2(input)
            .expect("parse punctuated shutdown args");
        let mut iter = args.into_iter();
        let msg_expr = iter.next().expect("message arg");
        let clock_expr = iter.next().expect("clock arg");

        let lit: LitStr = match msg_expr {
            Expr::Lit(ExprLit {
                lit: Lit::Str(s), ..
            }) => s,
            _ => panic!("not a str literal"),
        };
        let hash_ident = static_string_hash_ident(&lit.value(), lit.span());
        quote! {
            <_ as ::ankyra::SendReply<::ankyra::Shutdown>>::send(
                __ankyra_sender,
                ::ankyra::Shutdown {
                    clock: #clock_expr,
                    static_string_id: crate::_ankyra_config::static_strings::#hash_ident,
                },
            )
        }
    }

    #[test]
    fn shutdown_emits_sendreply_with_static_string_path() {
        let out = expand_shutdown_for_test(quote!("probe", 0u32)).to_string();
        assert!(
            out.contains(":: ankyra :: SendReply < :: ankyra :: Shutdown >"),
            "missing turbofish SendReply<Shutdown>: {out}"
        );
        assert!(
            out.contains("__ankyra_sender"),
            "missing __ankyra_sender: {out}"
        );
        assert!(out.contains("clock : 0u32"), "clock expr wrong: {out}");
        assert!(
            out.contains("static_string_id : crate :: _ankyra_config :: static_strings :: __ANKYRA_SS_f97691246db266f1"),
            "static_string_id path wrong: {out}"
        );
    }

    fn expand_shutdown_from_for_test(input: TokenStream2) -> TokenStream2 {
        let parsed: ShutdownFromCallSite = syn::parse2(input).expect("parse ShutdownFromCallSite");
        expand_shutdown_from_call_site_impl(&parsed)
    }

    #[test]
    fn shutdown_from_binds_sender_expr_then_sends() {
        let out = expand_shutdown_from_for_test(quote!(&mut sender, "probe", 0u32)).to_string();
        // Single-evaluation shim: bind the sender before the dispatch call.
        assert!(
            out.contains("let __ankyra_sender = & mut sender"),
            "missing single-evaluation shim: {out}"
        );
        assert!(
            out.contains(":: ankyra :: SendReply < :: ankyra :: Shutdown >"),
            "missing turbofish SendReply<Shutdown>: {out}"
        );
        assert!(out.contains("clock : 0u32"), "clock expr wrong: {out}");
        // Hash of `probe` is pinned in shared::fnv_tests.
        assert!(
            out.contains("static_string_id : crate :: _ankyra_config :: static_strings :: __ANKYRA_SS_f97691246db266f1"),
            "static_string_id path wrong: {out}"
        );
    }

    #[test]
    fn shutdown_from_complex_sender_expr_is_bound_once() {
        let out =
            expand_shutdown_from_for_test(quote!(transport.sender(), "probe", 0u32)).to_string();
        assert!(
            out.contains("let __ankyra_sender = transport . sender ()"),
            "sender expr not bound: {out}"
        );
        // `__ankyra_sender` should appear exactly twice: once in the
        // binding and once as the first arg to `send`.
        let occurrences = out.matches("__ankyra_sender").count();
        assert_eq!(
            occurrences, 2,
            "expected exactly two references to __ankyra_sender (binding + call site): {out}"
        );
    }

    #[test]
    fn shutdown_from_trailing_comma_accepted() {
        let out = expand_shutdown_from_for_test(quote!(&mut sender, "probe", 0u32,)).to_string();
        assert!(
            out.contains("let __ankyra_sender = & mut sender"),
            "trailing comma broke shim: {out}"
        );
    }

    #[test]
    fn shutdown_from_missing_sender_errors() {
        let result = syn::parse2::<ShutdownFromCallSite>(quote!("probe"));
        let Err(err) = result else {
            panic!("missing sender should fail");
        };
        let msg = err.to_string();
        assert!(
            msg.contains("klipper_shutdown_from!"),
            "diagnostic missing macro name: {msg}"
        );
    }

    #[test]
    fn shutdown_from_rejects_non_literal_reason() {
        let result = syn::parse2::<ShutdownFromCallSite>(quote!(&mut sender, reason_var, 0u32));
        let Err(err) = result else {
            panic!("non-literal reason should fail");
        };
        let msg = err.to_string();
        assert!(
            msg.contains("string literal"),
            "diagnostic should mention string literal: {msg}"
        );
    }
}
