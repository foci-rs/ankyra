//! `klipper_static_string!` and `klipper_shutdown!` function-like macros.
//!
//! # `klipper_static_string!("msg")`
//!
//! Expands to `crate::_ankyra_config::static_strings::__ANKYRA_SS_<hash>`.
//! The hash is the FNV-1a 64-bit digest of the literal's UTF-8 bytes as
//! computed by [`crate::shared::static_string_hash_ident`]; the same function
//! is used by the Task 12 assembler so both sides agree on the symbol name.
//!
//! The emitted path is plain `crate::…` (not `$crate::…`) because proc-macros
//! emit literal tokens that resolve against the call site's crate, which is
//! exactly the crate that will run `ankyra_config!` in Task 11 to declare the
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
//! that attribute's dispatch wrapper. Task 5's body-scan already recognises
//! `klipper_shutdown!` invocations and folds an `S: SendReply<Shutdown>`
//! bound onto the wrapper's generic.

use proc_macro::TokenStream;
use proc_macro_error2::{abort, abort_call_site};
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::{Expr, ExprLit, Lit, LitStr, Token};

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
}
