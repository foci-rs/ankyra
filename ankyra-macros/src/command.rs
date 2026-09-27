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
//!    assembler.
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
use quote::{ToTokens, format_ident, quote};
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{
    FnArg, Ident, ItemFn, Macro, Meta, Pat, PatType, Token, Type, TypeReference, parse_macro_input,
};

use crate::shared::{carrier_ident, dispatch_ident, format_const_ident, name_const_ident};

enum ContextBinding {
    ViewTrait(TokenStream2),
    Concrete(TokenStream2),
}

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

struct CommandArg {
    binding: Ident,
    ty: TokenStream2,
    spec: &'static str,
}

/// Handler authors commonly prefix unused parameters with a leading
/// underscore to silence the `unused_variables` lint (e.g. `_oid: u8`).
/// That underscore is a Rust-level convention and must not leak into the
/// Klipper data-dictionary format string — Klipper's host compares the
/// format string byte-for-byte against its own `DECL_COMMAND` shape and
/// rejects a mismatch with `Command format mismatch`.
fn protocol_param_name(ident: &str) -> &str {
    if let Some(rest) = ident.strip_prefix('_') {
        if !rest.is_empty() && !rest.starts_with('_') {
            return rest;
        }
    }
    ident
}

fn command_arg_spec(ty: &Type) -> Option<&'static str> {
    match ty {
        Type::Path(tp) => {
            if tp.qself.is_some() {
                return None;
            }
            let ident = tp.path.get_ident()?.to_string();
            Some(match ident.as_str() {
                "u32" => "%u",
                "u16" => "%hu",
                "u8" | "bool" => "%c",
                "i32" => "%i",
                "i16" => "%hi",
                _ => return None,
            })
        }
        Type::Reference(tr) => {
            if tr.mutability.is_some() {
                return None;
            }
            match tr.elem.as_ref() {
                Type::Slice(slice) => match slice.elem.as_ref() {
                    Type::Path(tp) if tp.path.is_ident("u8") => Some("%*s"),
                    _ => None,
                },
                Type::Path(tp) if tp.path.is_ident("str") => Some("%.*s"),
                _ => None,
            }
        }
        _ => None,
    }
}

fn collect_command_args(item: &ItemFn) -> Vec<CommandArg> {
    let mut out = Vec::new();
    for arg in item.sig.inputs.iter().skip(1) {
        let pat_type = match arg {
            FnArg::Typed(pt) => pt,
            FnArg::Receiver(_) => abort!(arg, "klipper_command does not support `self` receivers"),
        };
        let binding = arg_binding_ident(pat_type);
        let spec = supported_arg_spec(&binding, pat_type.ty.as_ref());
        out.push(CommandArg {
            binding,
            ty: pat_type.ty.to_token_stream(),
            spec,
        });
    }
    out
}

fn arg_binding_ident(pat_type: &PatType) -> Ident {
    match pat_type.pat.as_ref() {
        Pat::Ident(pi) => pi.ident.clone(),
        _ => abort!(
            pat_type.pat,
            "klipper_command arguments must use a simple ident pattern (got destructuring pattern)"
        ),
    }
}

fn supported_arg_spec(binding: &Ident, ty: &Type) -> &'static str {
    if let Some(spec) = command_arg_spec(ty) {
        return spec;
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

enum SenderBound {
    Reply(TokenStream2),
    Output(TokenStream2),
    Shutdown,
}

struct BoundCollector {
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
        // Deliberately not descending into macro token streams: a nested
        // `klipper_reply!` inside a non-klipper macro body is opaque until
        // that macro expands.
        let Some(seg) = mac.path.segments.last() else {
            return;
        };
        let ident = seg.ident.to_string();
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
            "klipper_shutdown" => self.record(SenderBound::Shutdown),
            _ => {}
        }
    }
}

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
///
/// The attribute accepts an optional argument list:
///
/// * `#[klipper_command]` — the default. Commands are dropped by the
///   generated dispatcher when the firmware's context reports
///   `ShutdownState::is_shutdown() == true`.
/// * `#[klipper_command(in_shutdown)]` — mark the command as callable
///   while the MCU is in shutdown (status/recovery commands like
///   `get_clock`, `emergency_stop`, `clear_shutdown`). The dispatcher
///   skips the shutdown gate for such commands.
///
/// Any other argument (unknown ident, literal, structured meta) is
/// rejected with a span-accurate diagnostic at expansion time.
pub fn expand_command(attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_fn = parse_macro_input!(item as ItemFn);
    let attr_ts: TokenStream2 = attr.into();
    let in_shutdown = match parse_in_shutdown_attr(attr_ts) {
        Ok(v) => v,
        Err(err) => return err.to_compile_error().into(),
    };
    expand_command_impl(&item_fn, in_shutdown).into()
}

fn parse_in_shutdown_attr(attr: TokenStream2) -> syn::Result<bool> {
    if attr.is_empty() {
        return Ok(false);
    }
    let metas: Punctuated<Meta, Token![,]> =
        syn::parse::Parser::parse2(Punctuated::<Meta, Token![,]>::parse_terminated, attr)?;
    let mut in_shutdown = false;
    for meta in &metas {
        match meta {
            Meta::Path(path) if path.is_ident("in_shutdown") => {
                if in_shutdown {
                    return Err(syn::Error::new_spanned(
                        path,
                        "duplicate `in_shutdown` option on `#[klipper_command]`",
                    ));
                }
                in_shutdown = true;
            }
            other => {
                let rendered = other.to_token_stream().to_string();
                return Err(syn::Error::new_spanned(
                    other,
                    format!(
                        "unknown `#[klipper_command]` option `{rendered}`; \
                         expected `in_shutdown` or an empty attribute list"
                    ),
                ));
            }
        }
    }
    Ok(in_shutdown)
}

fn expand_command_impl(item_fn: &ItemFn, in_shutdown: bool) -> TokenStream2 {
    let binding = context_binding(item_fn);
    let args = collect_command_args(item_fn);

    let mut collector = BoundCollector::new();
    visit::visit_block(&mut collector, &item_fn.block);

    let handler_name = &item_fn.sig.ident;
    let dispatch_name = dispatch_ident(handler_name);

    let dispatch_generics = quote!(<S>);
    let where_clause = sender_where_clause(&collector);
    let dispatch = dispatch_fn(handler_name, &dispatch_name, &binding, &args, &where_clause);

    let name_str = handler_name.to_string();
    let message_format = command_message_format(&name_str, &args);
    let consts = sibling_consts(handler_name, &name_str, &message_format, in_shutdown);
    let carrier = command_carrier(handler_name, &dispatch_name, &name_str, &message_format);

    let rewritten_handler = rewrite_handler_with_sender(item_fn, &dispatch_generics, &where_clause);

    quote! {
        #rewritten_handler
        #dispatch
        #consts
        #carrier
    }
}

fn sender_where_clause(collector: &BoundCollector) -> TokenStream2 {
    let sender_bounds: Vec<TokenStream2> = collector
        .bounds
        .values()
        .map(|b| match b {
            SenderBound::Reply(ty) => quote!(S: ::ankyra::SendReply<#ty>),
            SenderBound::Output(ty) => quote!(S: ::ankyra::SendOutput<#ty>),
            SenderBound::Shutdown => quote!(S: ::ankyra::SendReply<::ankyra::Shutdown>),
        })
        .collect();
    if sender_bounds.is_empty() {
        quote!()
    } else {
        quote!(where #(#sender_bounds),*)
    }
}

fn dispatch_fn(
    handler_name: &Ident,
    dispatch_name: &Ident,
    binding: &ContextBinding,
    args: &[CommandArg],
    where_clause: &TokenStream2,
) -> TokenStream2 {
    // A view-trait context is taken as `&mut dyn Trait` rather than a
    // `C: Trait + ?Sized` generic: a `?Sized` generic cannot be coerced to
    // `&mut dyn Trait` at the handler call, and a `Sized` one makes a bad
    // context fail on the coercion instead of on the view bound. With
    // `dyn`, the caller's `&mut State` coercion checks `State: Trait`.
    let ctx_param_ty = match binding {
        // Parenthesized because `&mut dyn Trait + '_` is ambiguous.
        ContextBinding::ViewTrait(bounds) => quote!((dyn #bounds + '_)),
        ContextBinding::Concrete(ty) => ty.clone(),
    };

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
    quote! {
        #[doc(hidden)]
        #[allow(non_snake_case)]
        pub fn #dispatch_name <S> (
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
    }
}

/// "<name>[ <arg>=%<spec>]*" -- Klipper's host decodes commands with this
/// exact string, so it must match `DECL_COMMAND`'s shape byte-for-byte.
fn command_message_format(name_str: &str, args: &[CommandArg]) -> String {
    let mut message_format = name_str.to_string();
    for arg in args {
        let binding_str = arg.binding.to_string();
        let wire_name = protocol_param_name(&binding_str);
        message_format.push(' ');
        message_format.push_str(wire_name);
        message_format.push('=');
        message_format.push_str(arg.spec);
    }
    message_format
}

/// Sibling `pub const`s (not carrier-macro arms) so the assembler can reach
/// them by reconstructed `crate::...` path without tripping
/// rust-lang/rust#52234.
fn sibling_consts(
    handler_name: &Ident,
    name_str: &str,
    message_format: &str,
    in_shutdown: bool,
) -> TokenStream2 {
    let name_const_name = name_const_ident("command", handler_name);
    let format_const_name = format_const_ident("command", handler_name);
    let in_shutdown_const_name = format_ident!("__ANKYRA_IN_SHUTDOWN_{}", handler_name);
    quote! {
        #[doc(hidden)]
        #[allow(non_upper_case_globals)]
        pub const #name_const_name: &str = #name_str;
        #[doc(hidden)]
        #[allow(non_upper_case_globals)]
        pub const #format_const_name: &str = #message_format;
        #[doc(hidden)]
        #[allow(non_upper_case_globals)]
        pub const #in_shutdown_const_name: bool = #in_shutdown;
    }
}

fn command_carrier(
    handler_name: &Ident,
    dispatch_name: &Ident,
    name_str: &str,
    message_format: &str,
) -> TokenStream2 {
    let carrier_name = carrier_ident("command", handler_name);
    quote! {
        #[doc(hidden)]
        #[macro_export]
        macro_rules! #carrier_name {
            () => {
                (command, #name_str, #message_format, $crate::#dispatch_name)
            };
        }
    }
}

/// Function parameters are visible through normal lexical scope, so the
/// `__ankyra_sender` reference emitted by `klipper_reply!` et al. resolves
/// across proc-macro hygiene boundaries; a `let` emitted by a different
/// expansion would not.
fn rewrite_handler_with_sender(
    item_fn: &ItemFn,
    dispatch_generics: &TokenStream2,
    where_clause: &TokenStream2,
) -> TokenStream2 {
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
            let _ = &__ankyra_sender;
            #body
        }
    }
}

#[cfg(test)]
fn expand_for_test(input: TokenStream2) -> TokenStream2 {
    let item_fn: ItemFn = syn::parse2(input).expect("failed to parse test input as ItemFn");
    expand_command_impl(&item_fn, false)
}

#[cfg(test)]
fn expand_for_test_in_shutdown(input: TokenStream2) -> TokenStream2 {
    let item_fn: ItemFn = syn::parse2(input).expect("failed to parse test input as ItemFn");
    expand_command_impl(&item_fn, true)
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
        let where_count = out
            .matches("where S : :: ankyra :: SendReply < Pong >")
            .count();
        assert_eq!(
            where_count, 2,
            "expected exactly one dedup'd where-clause per emitted fn (handler + dispatch): {out}"
        );
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
        assert!(
            out.contains("set_timer (ctx , sender , oid , ticks)"),
            "expected dispatch to invoke handler with sender: {out}"
        );
        assert!(
            out.contains("__ankyra_sender : & mut S"),
            "expected __ankyra_sender parameter on rewritten handler: {out}"
        );
        assert!(
            !out.contains("let _ = & frame"),
            "frame silencer should be removed: {out}"
        );
    }

    #[test]
    fn default_command_emits_false_shutdown_const() {
        let input = quote! {
            fn emergency_stop(_ctx: &mut State) {}
        };
        let out = render(&expand_for_test(input));
        assert!(
            out.contains("pub const __ANKYRA_IN_SHUTDOWN_emergency_stop : bool = false"),
            "expected sibling bool const defaulting to false: {out}"
        );
    }

    #[test]
    fn in_shutdown_command_emits_true_shutdown_const() {
        let input = quote! {
            fn get_clock(_ctx: &mut State) {}
        };
        let out = render(&expand_for_test_in_shutdown(input));
        assert!(
            out.contains("pub const __ANKYRA_IN_SHUTDOWN_get_clock : bool = true"),
            "expected sibling bool const set to true: {out}"
        );
    }

    #[test]
    fn parse_in_shutdown_attr_empty_is_false() {
        let result = super::parse_in_shutdown_attr(quote!()).expect("empty attr parses");
        assert!(!result);
    }

    #[test]
    fn parse_in_shutdown_attr_accepts_ident() {
        let result =
            super::parse_in_shutdown_attr(quote!(in_shutdown)).expect("in_shutdown parses");
        assert!(result);
    }

    #[test]
    fn parse_in_shutdown_attr_rejects_unknown_ident() {
        let err = super::parse_in_shutdown_attr(quote!(whatever))
            .expect_err("unknown ident must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("unknown `#[klipper_command]` option"),
            "wrong diagnostic: {msg}"
        );
    }

    #[test]
    fn parse_in_shutdown_attr_rejects_namevalue() {
        let err = super::parse_in_shutdown_attr(quote!(in_shutdown = true))
            .expect_err("name=value form must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("unknown `#[klipper_command]` option"),
            "wrong diagnostic: {msg}"
        );
    }

    #[test]
    fn parse_in_shutdown_attr_rejects_duplicate() {
        let err = super::parse_in_shutdown_attr(quote!(in_shutdown, in_shutdown))
            .expect_err("duplicates must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("duplicate `in_shutdown`"),
            "wrong diagnostic: {msg}"
        );
    }

    #[test]
    fn protocol_param_name_strips_single_leading_underscore() {
        assert_eq!(super::protocol_param_name("oid"), "oid");
        assert_eq!(super::protocol_param_name("_oid"), "oid");
        assert_eq!(super::protocol_param_name("count"), "count");
        assert_eq!(super::protocol_param_name("_count"), "count");
    }

    #[test]
    fn protocol_param_name_preserves_double_underscore() {
        assert_eq!(super::protocol_param_name("__oid"), "__oid");
        assert_eq!(super::protocol_param_name("___triple"), "___triple");
    }

    #[test]
    fn protocol_param_name_preserves_bare_underscore() {
        assert_eq!(super::protocol_param_name("_"), "_");
    }

    #[test]
    fn underscore_prefixed_args_strip_in_wire_format() {
        let input = quote! {
            fn set_pin(_ctx: &mut State, _oid: u8, value: u8) {
                let _ = (_oid, value);
            }
        };
        let out = render(&expand_for_test(input));
        assert!(
            out.contains("\"set_pin oid=%c value=%c\""),
            "expected stripped wire format: {out}"
        );
        assert!(
            out.contains("_oid : u8"),
            "handler binding must remain `_oid` to silence the lint: {out}"
        );
        assert!(
            out.contains("let _oid = < u8 as :: ankyra :: encoding :: Readable > :: read"),
            "dispatch wrapper must read into the handler's `_oid` binding: {out}"
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
