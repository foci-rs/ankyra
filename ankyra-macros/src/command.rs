//! `#[klipper_command]` attribute expansion.
//!
//! The attribute wraps a handler function of the form
//!
//! ```ignore
//! #[klipper_command]
//! fn get_clock(ctx: &mut dyn ClockCtxView) {
//!     // optional klipper_reply!/klipper_output!/klipper_shutdown! invocations
//! }
//! ```
//!
//! and emits three sibling items:
//!
//! 1. The original handler, passed through unchanged.
//! 2. A dispatch wrapper `__ankyra_dispatch_<name>` whose generics and
//!    where-clause encode the context trait (when the handler takes
//!    `&mut dyn Trait`) plus one [`SendReply<T>`] / [`SendOutput<T>`] bound
//!    per direct `klipper_reply!` / `klipper_output!` / `klipper_shutdown!`
//!    invocation found inside the handler body.
//! 3. A `#[macro_export]` carrier macro `__ankyra_item_command_<name>!`
//!    whose expansion yields a literal descriptor tuple consumed by the
//!    Task 10 assembler.
//!
//! # Body-scan restriction
//!
//! Bound collection only walks the handler body directly; helper function
//! bodies are not traversed. Therefore any `klipper_reply!`, `klipper_output!`,
//! or `klipper_shutdown!` invocation that should contribute a sender bound
//! must appear inline in the handler — not inside a helper that the handler
//! happens to call.
//!
//! Closures declared inline in the body are traversed (via
//! [`syn::visit::visit_block`]) so a reply emitted from within a closure
//! still contributes its bound.

use std::collections::BTreeMap;

use proc_macro::TokenStream;
use proc_macro_error2::abort;
use proc_macro2::TokenStream as TokenStream2;
use quote::{ToTokens, quote};
use syn::visit::{self, Visit};
use syn::{FnArg, ItemFn, Macro, PatType, Type, TypeReference, parse_macro_input};

use crate::shared::{carrier_ident, dispatch_ident};

/// Whether the command's first argument pins a view trait (`&mut dyn T`)
/// or a concrete receiver (`&mut T`).
///
/// For `ViewTrait` the stored token stream is the bound list (without the
/// `dyn` keyword) suitable for splicing into a where-clause as
/// `C: <bounds> + ?Sized`.
enum ContextBinding {
    ViewTrait(TokenStream2),
    Concrete(TokenStream2),
}

/// Extract the context binding from a `#[klipper_command]` handler.
///
/// Aborts with a span-pointed diagnostic if the first argument is missing,
/// is `self`, is an owned type, or is a shared reference.
fn context_binding(item: &ItemFn) -> ContextBinding {
    let Some(first) = item.sig.inputs.first() else {
        abort!(
            item.sig.ident,
            "klipper_command requires a first context argument (&mut T or &mut dyn Trait)"
        );
    };
    let arg_ty = match first {
        FnArg::Typed(PatType { ty, .. }) => ty.as_ref(),
        FnArg::Receiver(_) => abort!(first, "klipper_command does not support `self` receivers"),
    };
    match arg_ty {
        Type::Reference(TypeReference {
            mutability, elem, ..
        }) => {
            if mutability.is_none() {
                abort!(
                    arg_ty,
                    "klipper_command context argument must be `&mut T` or `&mut dyn Trait`; \
                     shared references are not allowed"
                );
            }
            match elem.as_ref() {
                Type::TraitObject(obj) => {
                    // Emit just the bound list (e.g. `ClockCtxView`) without
                    // the `dyn` keyword so we can splice it into a
                    // where-clause: `C: <bounds> + ?Sized`.
                    let bounds = &obj.bounds;
                    ContextBinding::ViewTrait(quote!(#bounds))
                }
                _ => ContextBinding::Concrete(elem.as_ref().to_token_stream()),
            }
        }
        _ => abort!(
            arg_ty,
            "klipper_command context argument must be `&mut T` or `&mut dyn Trait`"
        ),
    }
}

/// Sender bound discovered by the body-scan visitor.
///
/// The three variants mirror the three sender-consuming macros. Bounds are
/// stored in a `BTreeMap` keyed by the stringified payload type so that
/// duplicates collapse and the emitted where-clause is deterministic.
enum SenderBound {
    Reply(TokenStream2),
    Output(TokenStream2),
    Shutdown,
}

/// Visitor that walks a handler body and records every direct
/// `klipper_reply!(T, ..)`, `klipper_output!(T, ..)`, or `klipper_shutdown!`
/// invocation. The first token-group of the arguments (up to the first
/// top-level comma) is treated as the payload type `T`.
///
/// Only the last path segment of the macro is matched, so
/// `::ankyra::klipper_reply!` and `crate::klipper_reply!` and plain
/// `klipper_reply!` all count. A hypothetical `klipper_reply_v2!` would
/// not match because the ident differs.
struct BoundCollector {
    /// key = rendered token string (for dedup), value = bound variant.
    bounds: BTreeMap<String, SenderBound>,
}

impl BoundCollector {
    fn new() -> Self {
        Self {
            bounds: BTreeMap::new(),
        }
    }

    fn record(&mut self, bound: SenderBound) {
        let key = match &bound {
            SenderBound::Reply(ty) => format!("R:{ty}"),
            SenderBound::Output(ty) => format!("O:{ty}"),
            SenderBound::Shutdown => "S".to_string(),
        };
        self.bounds.entry(key).or_insert(bound);
    }
}

impl<'ast> Visit<'ast> for BoundCollector {
    fn visit_macro(&mut self, mac: &'ast Macro) {
        // We intentionally do not descend into macro token streams via
        // `visit::visit_macro` defaults; a nested `klipper_reply!` inside
        // a non-klipper macro body is opaque until that macro expands.
        let ident = match mac.path.segments.last() {
            Some(seg) => seg.ident.to_string(),
            None => return,
        };
        match ident.as_str() {
            "klipper_reply" => {
                if let Some(ty) = first_type_token(mac.tokens.clone()) {
                    self.record(SenderBound::Reply(ty));
                }
            }
            "klipper_output" => {
                if let Some(ty) = first_type_token(mac.tokens.clone()) {
                    self.record(SenderBound::Output(ty));
                }
            }
            "klipper_shutdown" => {
                // `klipper_shutdown!("msg", clock)` — bound is fixed to
                // `S: SendReply<::ankyra::Shutdown>`. No payload extraction
                // required.
                self.record(SenderBound::Shutdown);
            }
            _ => {}
        }
    }
}

/// Extract the first top-level token group from a macro argument stream,
/// i.e. everything before the first `,` at depth 0. Returns `None` if the
/// stream is empty.
fn first_type_token(tokens: TokenStream2) -> Option<TokenStream2> {
    let mut out = TokenStream2::new();
    for tt in tokens {
        if let proc_macro2::TokenTree::Punct(p) = &tt {
            if p.as_char() == ',' && p.spacing() == proc_macro2::Spacing::Alone {
                break;
            }
        }
        out.extend(std::iter::once(tt));
    }
    if out.is_empty() { None } else { Some(out) }
}

/// Entry point for `#[klipper_command]` expansion.
pub fn expand_command(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_fn = parse_macro_input!(item as ItemFn);
    expand_command_impl(&item_fn).into()
}

fn expand_command_impl(item_fn: &ItemFn) -> TokenStream2 {
    let binding = context_binding(item_fn);

    let mut collector = BoundCollector::new();
    visit::visit_block(&mut collector, &item_fn.block);

    let handler_name = &item_fn.sig.ident;
    let dispatch_name = dispatch_ident(handler_name);
    let carrier_name = carrier_ident("command", handler_name);

    // Build sender bounds from the collector. Order is deterministic
    // because `BTreeMap` iterates in key order.
    let sender_bounds: Vec<TokenStream2> = collector
        .bounds
        .values()
        .map(|b| match b {
            SenderBound::Reply(ty) => quote!(S: ::ankyra::SendReply<#ty>),
            SenderBound::Output(ty) => quote!(S: ::ankyra::SendOutput<#ty>),
            SenderBound::Shutdown => quote!(S: ::ankyra::SendReply<::ankyra::Shutdown>),
        })
        .collect();

    // For the view-trait binding we take `&mut dyn Trait` directly rather
    // than adding a `C: Trait + ?Sized` generic. Rationale: a `?Sized`
    // context generic cannot be coerced to `&mut dyn Trait` at the
    // handler call site (unsized coercion requires `C: Sized`), and a
    // `Sized` context generic forces the trybuild fixture
    // `command_bad_context.rs` to name the wrong failure (E0277 on
    // coercion instead of E0277 on the view bound). Taking `&mut dyn
    // Trait` at the dispatch boundary pushes the bound check to the
    // caller, where the user's `&mut State` is coerced into the trait
    // object and `State: ClockCtxView` is checked as a side effect.
    let (dispatch_generics, where_clause, ctx_param_ty) = match binding {
        ContextBinding::ViewTrait(bounds) => {
            let where_clause = if sender_bounds.is_empty() {
                quote!()
            } else {
                quote!(where #(#sender_bounds),*)
            };
            // Wrap in parens because `&mut dyn Trait + '_` is a parse
            // ambiguity — `&mut (dyn Trait + '_)` disambiguates.
            (quote!(<S>), where_clause, quote!((dyn #bounds + '_)))
        }
        ContextBinding::Concrete(ty) => {
            let where_clause = if sender_bounds.is_empty() {
                quote!()
            } else {
                quote!(where #(#sender_bounds),*)
            };
            (quote!(<S>), where_clause, ty)
        }
    };

    // The dispatch wrapper. For Task 5 the body has zero extra args — we
    // silence the unused-variable lint on `__ankyra_sender` and on the
    // `frame` slice explicitly because empty-body commands do not consume
    // them. Later tasks will wire in Readable::read for additional args
    // and thread replies through `__ankyra_sender`.
    let dispatch = quote! {
        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #dispatch_name #dispatch_generics (
            frame: &mut &[u8],
            ctx: &mut #ctx_param_ty,
            sender: &mut S,
        ) -> ::core::result::Result<(), ::ankyra::encoding::ReadError>
        #where_clause
        {
            let __ankyra_sender: &mut S = sender;
            let _ = &__ankyra_sender;
            let _ = &frame;
            #handler_name(ctx);
            ::core::result::Result::Ok(())
        }
    };

    // Carrier macro. The tuple shape is provisional and coordinated with
    // Task 10's `__ankyra_assemble!` accumulator:
    //
    //   (kind_literal, protocol_name, message_format, dispatch_fn_path)
    //
    // For now:
    //   * kind_literal is the bare ident `command` so the accumulator can
    //     match on it as a keyword.
    //   * protocol_name and message_format are both `stringify!(<name>)`.
    //     Task 5b will refine message_format to the fully rendered
    //     "name param1=%u param2=%s" string once argument deserialization
    //     lands.
    //   * dispatch_fn_path uses `$crate::__ankyra_dispatch_<name>`. This
    //     resolves correctly when the handler is defined at the defining
    //     crate's root (the trybuild fixtures). Full-module-path handlers
    //     will be refined in Task 9 when provider companion macros land.
    let name_str = handler_name.to_string();
    let carrier = quote! {
        #[doc(hidden)]
        #[macro_export]
        macro_rules! #carrier_name {
            () => {
                (command, #name_str, #name_str, $crate::#dispatch_name)
            };
        }
    };

    quote! {
        #item_fn
        #dispatch
        #carrier
    }
}

/// Test helper: drive `expand_command_impl` from a `TokenStream2` and
/// return the rendered output. Used by unit tests below and kept crate-
/// private.
#[cfg(test)]
fn expand_for_test(input: TokenStream2) -> TokenStream2 {
    let item_fn: ItemFn = syn::parse2(input).expect("failed to parse test input as ItemFn");
    expand_command_impl(&item_fn)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(ts: &TokenStream2) -> String {
        ts.to_string()
    }

    #[test]
    fn view_trait_emits_dyn_ctx() {
        let input = quote! {
            fn get_clock(_ctx: &mut dyn ClockCtxView) {}
        };
        let out = render(&expand_for_test(input));
        assert!(
            out.contains("__ankyra_dispatch_get_clock"),
            "dispatch ident missing: {out}"
        );
        // View-trait handlers expose a `ctx: &mut dyn Trait + '_` parameter
        // rather than a `C: Trait + ?Sized` generic — see the inline comment
        // in `expand_command_impl` for why.
        assert!(out.contains("dyn ClockCtxView"), "dyn ctx missing: {out}");
        assert!(
            out.contains("__ankyra_item_command_get_clock"),
            "carrier missing: {out}"
        );
    }

    #[test]
    fn concrete_omits_view_bound() {
        let input = quote! {
            fn emergency_stop(_ctx: &mut State) {}
        };
        let out = render(&expand_for_test(input));
        assert!(
            out.contains("ctx : & mut State"),
            "concrete ctx missing: {out}"
        );
        // No where-clause should be emitted for an empty-body concrete command.
        assert!(!out.contains("where"), "unexpected where-clause: {out}");
    }

    #[test]
    fn reply_in_body_adds_send_reply_bound() {
        let input = quote! {
            fn ping(_ctx: &mut State) {
                klipper_reply!(Pong, tag = 0u32);
            }
        };
        let out = render(&expand_for_test(input));
        assert!(
            out.contains("SendReply < Pong >"),
            "expected SendReply<Pong> bound: {out}"
        );
    }

    #[test]
    fn output_in_body_adds_send_output_bound() {
        let input = quote! {
            fn stream(_ctx: &mut State) {
                klipper_output!(Tick, t = 0u32);
            }
        };
        let out = render(&expand_for_test(input));
        assert!(
            out.contains("SendOutput < Tick >"),
            "expected SendOutput<Tick> bound: {out}"
        );
    }

    #[test]
    fn shutdown_in_body_adds_shutdown_bound() {
        let input = quote! {
            fn halt(_ctx: &mut State) {
                klipper_shutdown!("boom", 0u32);
            }
        };
        let out = render(&expand_for_test(input));
        assert!(
            out.contains(":: ankyra :: Shutdown"),
            "expected Shutdown bound: {out}"
        );
    }

    #[test]
    fn duplicate_replies_dedup() {
        let input = quote! {
            fn twin(_ctx: &mut State) {
                klipper_reply!(Pong, a = 0u32);
                klipper_reply!(Pong, b = 1u32);
            }
        };
        let out = render(&expand_for_test(input));
        let count = out.matches("SendReply < Pong >").count();
        assert_eq!(count, 1, "expected dedup, got {count} in: {out}");
    }
}
