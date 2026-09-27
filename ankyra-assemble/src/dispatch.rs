//! Emit the firmware's `Config` impl and the command dispatch match arms.
//!
//! The `Transport<C>` contract in `ankyra/src/transport.rs` requires one
//! impl of `ankyra::transport::Config`: it names the outbound
//! `TransportOutput`, the per-call `Context<'c>`, and a `dispatch`
//! function that takes a u16 command id, a frame cursor, and the context
//! and routes the frame to the correct handler.
//!
//! This module emits a minimal `pub struct Config;` and the matching
//! impl. The `dispatch` match arms:
//!
//! * id 0 (synthesized `identify_response`) → protocol error. The MCU
//!   never receives its own reply as a command; fall through to
//!   `Err(ReadError)` to trip the transport's resync logic.
//! * id 1 (synthesized `identify`) → call `handle_identify`.
//! * id N (user command) → call `<prefix>::__ankyra_dispatch_<name>(frame,
//!   ctx, &mut Sender)`.
//! * anything else → `Err(ReadError)` to force resync.
//!
//! # Lifetime rewrite
//!
//! The user writes `context = &'ctx mut T` in `ankyra_config!` for
//! readability. The `Config::Context<'c>` GAT demands the lifetime be
//! named `'c`. This module therefore rewrites every `'ctx` lifetime
//! token to `'c` before splicing the type into the impl. No other
//! lifetime names are touched; if a user passes something else (e.g. a
//! concrete named lifetime like `'static`) it flows through verbatim.

use proc_macro2::{Group, TokenStream as TokenStream2, TokenTree};
use quote::quote;

use crate::identify::{IDENTIFY_CMD_ID, IDENTIFY_CMD_NAME, IDENTIFY_RESPONSE_REPLY_ID};
use crate::sort::{AssembledItem, Assembly, ItemKind};

/// Construct the path to a command's `__ANKYRA_IN_SHUTDOWN_<name>` sibling
/// const.
///
/// Uses `sibling_scope` when present. Otherwise the path is derived from
/// the dispatch-fn path, since `#[klipper_command]` emits the dispatch fn
/// and the const side by side. Inline-tuple fixtures with neither yield
/// `false`, so the gate always routes to the handler.
fn in_shutdown_const_path(item: &AssembledItem) -> TokenStream2 {
    let const_ident_str = format!("__ANKYRA_IN_SHUTDOWN_{}", item.name);
    if let Some(prefix) = &item.sibling_scope {
        let const_ident: syn::Ident = syn::parse_str(&const_ident_str)
            .expect("__ANKYRA_IN_SHUTDOWN_<name> is always a valid ident");
        return quote!(#prefix::#const_ident);
    }
    if let Some(dispatch) = &item.dispatch_path {
        if let Some(path) = swap_trailing_segment(dispatch, &const_ident_str) {
            return path;
        }
    }
    quote!(false)
}

/// Swap the last path segment of `path` for `new_ident`. Returns `None` if
/// `path` is not a `syn::Path` (inline-tuple fixtures may pass arbitrary
/// tokens as the dispatch target).
fn swap_trailing_segment(path: &TokenStream2, new_ident: &str) -> Option<TokenStream2> {
    let mut out: syn::Path = syn::parse2(path.clone()).ok()?;
    let last = out.segments.last_mut()?;
    last.ident = syn::parse_str(new_ident).ok()?;
    last.arguments = syn::PathArguments::None;
    Some(quote!(#out))
}

/// Rewrite every `'ctx` lifetime to `'c` inside `tokens`, recursing into
/// groups. Lifetimes tokenize as `Punct('\'')` followed by an `Ident`.
fn rewrite_lifetime_ctx(tokens: TokenStream2) -> TokenStream2 {
    let mut out = TokenStream2::new();
    let mut prev_was_apostrophe = false;
    for tt in tokens {
        match tt {
            TokenTree::Punct(p) if p.as_char() == '\'' => {
                out.extend(std::iter::once(TokenTree::Punct(p)));
                prev_was_apostrophe = true;
            }
            TokenTree::Ident(ident) if prev_was_apostrophe => {
                if ident == "ctx" {
                    out.extend(std::iter::once(TokenTree::Ident(proc_macro2::Ident::new(
                        "c",
                        ident.span(),
                    ))));
                } else {
                    out.extend(std::iter::once(TokenTree::Ident(ident)));
                }
                prev_was_apostrophe = false;
            }
            TokenTree::Group(g) => {
                let inner = rewrite_lifetime_ctx(g.stream());
                let mut new = Group::new(g.delimiter(), inner);
                new.set_span(g.span());
                out.extend(std::iter::once(TokenTree::Group(new)));
                prev_was_apostrophe = false;
            }
            other => {
                out.extend(std::iter::once(other));
                prev_was_apostrophe = false;
            }
        }
    }
    out
}

/// Emit the `Config` struct plus the `impl ankyra::transport::Config for
/// Config` block. The caller supplies the transport output type and the
/// context type (both already parsed into `TokenStream2` form by
/// `input::parse`).
pub(crate) fn emit(
    assembly: &Assembly,
    transport_ty: &TokenStream2,
    context_ty: &TokenStream2,
) -> TokenStream2 {
    let ctx_referent = match syn::parse2::<syn::Type>(context_ty.clone()) {
        Ok(syn::Type::Reference(r)) if r.mutability.is_some() => quote!(&mut **ctx),
        _ => quote!(ctx),
    };
    let user_command_arms: Vec<TokenStream2> = assembly
        .items()
        .iter()
        .filter(|i| i.kind == ItemKind::Command && i.name != IDENTIFY_CMD_NAME)
        .map(|i| {
            let id = i.id;
            let path = i
                .dispatch_path
                .as_ref()
                .expect("command items must carry a dispatch path");
            let in_shutdown_const = in_shutdown_const_path(i);
            quote! {
                #id => {
                    if !#in_shutdown_const
                        && ::ankyra::ShutdownState::is_shutdown(ctx)
                    {
                        ::core::result::Result::Ok(())
                    } else {
                        #path(frame, #ctx_referent, &mut Sender)
                    }
                }
            }
        })
        .collect();

    let identify_response_id = IDENTIFY_RESPONSE_REPLY_ID;
    let identify_cmd_id = IDENTIFY_CMD_ID;
    let context_ty = rewrite_lifetime_ctx(context_ty.clone());

    quote! {
        /// Assembler-generated transport configuration.
        ///
        /// The firmware never constructs this; [`KLIPPER_TRANSPORT`] holds
        /// the single instance and threads dispatch through it.
        pub struct Config;

        impl ::ankyra::transport::Config for Config {
            type TransportOutput = #transport_ty;
            type Context<'c> = #context_ty;

            fn dispatch<'c>(
                cmd: u16,
                frame: &mut &[u8],
                ctx: &mut Self::Context<'c>,
            ) -> ::core::result::Result<(), ::ankyra::encoding::ReadError> {
                let _ = &ctx;
                match cmd {
                    #identify_response_id => ::core::result::Result::Err(
                        ::ankyra::encoding::ReadError
                    ),
                    #identify_cmd_id => handle_identify(frame, &mut Sender),
                    #(#user_command_arms)*
                    _ => ::core::result::Result::Err(::ankyra::encoding::ReadError),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::rewrite_lifetime_ctx;
    use quote::quote;

    #[test]
    fn rewrites_ctx_to_c() {
        let input = quote! { &'ctx mut () };
        let out = rewrite_lifetime_ctx(input).to_string();
        assert!(!out.contains("ctx"), "{out}");
        assert!(out.contains("'c"), "{out}");
    }

    #[test]
    fn preserves_other_lifetimes() {
        let input = quote! { &'static mut () };
        let out = rewrite_lifetime_ctx(input).to_string();
        assert!(out.contains("static"), "{out}");
    }

    #[test]
    fn walks_into_nested_groups() {
        let input = quote! { (&'ctx mut T, &'ctx mut U) };
        let out = rewrite_lifetime_ctx(input).to_string();
        assert!(!out.contains("ctx"), "{out}");
        assert_eq!(out.matches("'c").count(), 2, "{out}");
    }
}
