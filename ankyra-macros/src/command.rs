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
//! 1. The user's handler fn, rewritten to take `__ankyra_sender: &mut S` as
//!    its second formal parameter (immediately after the context) plus a
//!    `<S>` generic and a sender-bound `where`-clause. The body is spliced
//!    through unchanged so `klipper_reply!` / `klipper_output!` /
//!    `klipper_shutdown!` invocations inside it resolve `__ankyra_sender`
//!    through normal function-parameter scope rather than a local `let`
//!    emitted from a different proc-macro expansion — the latter would
//!    fail to cross proc-macro hygiene boundaries.
//! 2. A dispatch wrapper `__ankyra_dispatch_<name>` whose generics and
//!    where-clause mirror the rewritten handler's. It reads the
//!    serialized command arguments off the frame cursor and forwards
//!    `ctx`, `sender`, and those arguments to the rewritten handler.
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
use syn::{
    FnArg, Ident, ItemFn, Macro, Pat, PatType, Type, TypePath, TypeReference, parse_macro_input,
};

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

/// A deserializable argument parsed off the handler signature.
///
/// `binding` is the local binding ident emitted in the dispatch wrapper
/// (identical to the handler's ident, including any leading underscore).
/// `ty` is spliced verbatim as the `<T as Readable>::read(..)` generic.
struct CommandArg {
    binding: Ident,
    ty: TokenStream2,
}

/// Collect the deserializable arguments (everything after the context arg).
///
/// Each arg must be a typed `FnArg` (not a `self` receiver), bind a simple
/// ident pattern, and carry a type drawn from the supported allowlist:
/// `u8`, `u16`, `u32`, `i16`, `i32`, `bool`, `&[u8]`, `&str`. Named
/// lifetimes on slice/str args are permitted; other shapes are rejected
/// with a span-pointed diagnostic.
fn collect_command_args(item: &ItemFn) -> Vec<CommandArg> {
    let mut out = Vec::new();
    for arg in item.sig.inputs.iter().skip(1) {
        let pat_type = match arg {
            FnArg::Typed(pt) => pt,
            FnArg::Receiver(_) => abort!(arg, "klipper_command does not support `self` receivers"),
        };
        let binding = arg_binding_ident(pat_type);
        validate_supported_type(&binding, pat_type.ty.as_ref());
        out.push(CommandArg {
            binding,
            ty: pat_type.ty.to_token_stream(),
        });
    }
    out
}

/// Extract the binding ident from a `pat: Ty` argument.
///
/// Only plain ident patterns are supported; destructuring patterns (`(a, b)`,
/// `Foo { x }`, etc.) are rejected because their binding name cannot be
/// reused as-is in the generated `let <name> = ...` line.
fn arg_binding_ident(pat_type: &PatType) -> Ident {
    match pat_type.pat.as_ref() {
        Pat::Ident(pi) => pi.ident.clone(),
        _ => abort!(
            pat_type.pat,
            "klipper_command arguments must use a simple ident pattern (got destructuring pattern)"
        ),
    }
}

/// Abort expansion unless `ty` is in the supported allowlist.
///
/// The allowlist is:
/// - `u8`, `u16`, `u32`, `i16`, `i32`, `bool` (primitive idents)
/// - `&[u8]` / `&'a [u8]` (reference to a slice of `u8`)
/// - `&str` / `&'a str` (reference to the `str` primitive type)
///
/// Mutable references are rejected; the bytes under the cursor are
/// logically read-only for the duration of the dispatch call.
fn validate_supported_type(binding: &Ident, ty: &Type) {
    if is_supported_type(ty) {
        return;
    }
    let rendered = ty.to_token_stream().to_string();
    abort!(
        ty,
        "klipper_command argument `{}` has unsupported type `{}`. \
         Supported types: u8, u16, u32, i16, i32, bool, &[u8], &str.",
        binding,
        rendered
    );
}

fn is_supported_type(ty: &Type) -> bool {
    match ty {
        Type::Path(tp) => is_supported_primitive_path(tp),
        Type::Reference(tr) => is_supported_reference(tr),
        _ => false,
    }
}

fn is_supported_primitive_path(tp: &TypePath) -> bool {
    if tp.qself.is_some() {
        return false;
    }
    let Some(ident) = tp.path.get_ident() else {
        return false;
    };
    matches!(
        ident.to_string().as_str(),
        "u8" | "u16" | "u32" | "i16" | "i32" | "bool"
    )
}

fn is_supported_reference(tr: &TypeReference) -> bool {
    // Mutable references are never allowed: the dispatch wrapper must not
    // hand out a mutable view into the frame buffer.
    if tr.mutability.is_some() {
        return false;
    }
    match tr.elem.as_ref() {
        Type::Slice(slice) => match slice.elem.as_ref() {
            Type::Path(tp) => tp.path.is_ident("u8"),
            _ => false,
        },
        Type::Path(tp) => tp.path.is_ident("str"),
        _ => false,
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
    let args = collect_command_args(item_fn);

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

    // When the handler declares additional args, emit a `let <name> = <Ty
    // as Readable>::read(frame)?;` line per arg before invoking the
    // handler. With zero extra args the `frame` parameter is unused, so
    // we fall back to a `let _ = &frame;` silencer.
    let arg_reads: Vec<TokenStream2> = args
        .iter()
        .map(|arg| {
            let name = &arg.binding;
            let ty = &arg.ty;
            quote! {
                let #name = <#ty as ::ankyra::encoding::Readable>::read(frame)?;
            }
        })
        .collect();
    let arg_idents: Vec<&Ident> = args.iter().map(|arg| &arg.binding).collect();
    let frame_silencer = if args.is_empty() {
        quote!(let _ = &frame;)
    } else {
        quote!()
    };
    // The handler fn is rewritten below to take `__ankyra_sender: &mut S`
    // as an injected parameter (see `rewrite_handler_with_sender`). The
    // user-written body therefore sees `__ankyra_sender` through normal
    // function-parameter scope rather than via a proc-macro-emitted `let`.
    // This side-steps the cross-proc-macro hygiene issue that a local
    // `let` binding would hit — function parameters are visible across
    // independent proc-macro expansions because they live in normal Rust
    // scope, not in an expansion-local hygienic context.
    //
    // The dispatch wrapper therefore just reads args off the frame, forwards
    // `ctx` and `sender`, and invokes the rewritten handler.
    //
    // Visibility is `pub` because cross-crate aggregation (`ankyra_config!`
    // in a firmware crate referencing `clock_lib::__ankyra_dispatch_<name>`)
    // requires the dispatch fn to be reachable from the firmware crate. It
    // is still `#[doc(hidden)]` so it does not surface in user-facing docs.
    let dispatch = quote! {
        #[doc(hidden)]
        #[allow(non_snake_case)]
        pub fn #dispatch_name #dispatch_generics (
            frame: &mut &[u8],
            ctx: &mut #ctx_param_ty,
            sender: &mut S,
        ) -> ::core::result::Result<(), ::ankyra::encoding::ReadError>
        #where_clause
        {
            #frame_silencer
            #(#arg_reads)*
            #handler_name(ctx, sender, #(#arg_idents),*);
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

    // Rewrite the user's handler fn to inject `__ankyra_sender: &mut S` as
    // its second parameter (right after the context), and to carry the
    // `<S>` generic plus any sender `where`-clause bounds discovered by
    // the body-scan visitor. Reading `__ankyra_sender` from the body
    // therefore resolves through normal function-parameter scope, which
    // crosses proc-macro hygiene boundaries cleanly. Emitting the sender
    // as a local `let` in the dispatch wrapper would not — the user-
    // written `::ankyra::klipper_reply!(...)` inside the original
    // passthrough body would not see a dispatch-wrapper-local binding.
    let rewritten_handler = rewrite_handler_with_sender(item_fn, &dispatch_generics, &where_clause);

    quote! {
        #rewritten_handler
        #dispatch
        #carrier
    }
}

/// Return the user's handler fn with `__ankyra_sender: &mut S` injected as
/// its second formal parameter (immediately after the context), plus the
/// supplied `<S>` generics and sender-bound `where`-clause spliced onto
/// the signature.
///
/// This is how `__ankyra_sender` becomes visible inside the user body
/// across proc-macro hygiene boundaries (function parameters are visible
/// through normal lexical scope, unlike local `let` bindings emitted from
/// a different proc-macro expansion).
fn rewrite_handler_with_sender(
    item_fn: &ItemFn,
    dispatch_generics: &TokenStream2,
    where_clause: &TokenStream2,
) -> TokenStream2 {
    // Clone the attrs/vis/sig-prefix and splice in our injected sender
    // parameter after the first arg. The original inputs ordering is
    // preserved so that arg deserialization continues to line up.
    let attrs = &item_fn.attrs;
    let vis = &item_fn.vis;
    let name = &item_fn.sig.ident;
    let output = &item_fn.sig.output;
    let body = &item_fn.block;
    let inputs = &item_fn.sig.inputs;

    let mut inputs_iter = inputs.iter();
    let ctx_arg = inputs_iter
        .next()
        .expect("context_binding guarantees a first arg");
    let rest: Vec<_> = inputs_iter.collect();

    quote! {
        #(#attrs)*
        #vis fn #name #dispatch_generics (
            #ctx_arg,
            __ankyra_sender: &mut S,
            #(#rest),*
        ) #output
        #where_clause
        {
            // Silence the sender binding in handlers that never send a
            // reply or output; the body-scan still threaded the generic
            // through because users are allowed to add replies later
            // without re-running the bounds collection by hand.
            let _ = &__ankyra_sender;
            #body
        }
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
        // Two occurrences are expected: one on the rewritten handler's
        // where-clause and one on the dispatch wrapper's. Dedup is at the
        // bound level within a single where-clause — the emitted clauses
        // must not list `SendReply<Pong>` twice.
        let where_count = out
            .matches("where S : :: ankyra :: SendReply < Pong >")
            .count();
        assert_eq!(
            where_count, 2,
            "expected exactly one dedup'd where-clause per emitted fn (handler + dispatch): {out}"
        );
        // And no where-clause should list the bound twice.
        assert!(
            !out.contains("SendReply < Pong > , S : :: ankyra :: SendReply < Pong >"),
            "where-clause lists the same bound twice: {out}"
        );
    }

    #[test]
    fn args_emit_readable_reads_and_forwarded_call() {
        let input = quote! {
            fn set_timer(_ctx: &mut State, oid: u8, ticks: u32) {
                let _ = (oid, ticks);
            }
        };
        let out = render(&expand_for_test(input));
        assert!(
            out.contains("let oid = < u8 as :: ankyra :: encoding :: Readable > :: read (frame)"),
            "expected u8 read for oid: {out}"
        );
        assert!(
            out.contains(
                "let ticks = < u32 as :: ankyra :: encoding :: Readable > :: read (frame)"
            ),
            "expected u32 read for ticks: {out}"
        );
        // The dispatch wrapper calls the rewritten handler with sender
        // threaded in as the injected second parameter.
        assert!(
            out.contains("set_timer (ctx , sender , oid , ticks)"),
            "expected dispatch to invoke handler with sender: {out}"
        );
        // The rewritten handler exposes `__ankyra_sender` as a formal
        // parameter so the body can reference it without hygiene gymnastics.
        assert!(
            out.contains("__ankyra_sender : & mut S"),
            "expected __ankyra_sender parameter on rewritten handler: {out}"
        );
        // The frame silencer must be gone when args are present.
        assert!(
            !out.contains("let _ = & frame"),
            "frame silencer should be removed: {out}"
        );
    }

    #[test]
    fn slice_and_str_args_are_accepted() {
        let input = quote! {
            fn peek(_ctx: &mut State, buf: &[u8], label: &str) {}
        };
        let out = render(&expand_for_test(input));
        assert!(
            out.contains("& [u8] as :: ankyra :: encoding :: Readable"),
            "expected &[u8] read: {out}"
        );
        assert!(
            out.contains("& str as :: ankyra :: encoding :: Readable"),
            "expected &str read: {out}"
        );
    }
}
