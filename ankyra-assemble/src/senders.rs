//! Emit the firmware's `Sender` type and its `SendReply` / `SendOutput`
//! impls.
//!
//! # Shape
//!
//! The assembler emits a zero-sized `pub struct Sender;` paired with one
//! impl per aggregated reply or output payload type:
//!
//! ```ignore
//! impl ::ankyra::SendReply<R> for Sender {
//!     fn send(&mut self, payload: R) {
//!         // encode_frame closure emits u16 id, then payload bytes
//!     }
//! }
//! ```
//!
//! The hard-coded u16 id matches the id `sort::assemble` handed out for
//! that reply or output; the assembler threads it in at `quote!` time so
//! the firmware never pays a runtime lookup.
//!
//! # Built-in impls
//!
//! Two synthesized payloads always get impls regardless of user input:
//!
//! * `IdentifyResponse` (id 0) — the reply to the bootstrap `identify`
//!   command.
//! * `ankyra::Shutdown` at its assigned id — the firmware-wide unrecoverable
//!   fault signal.
//!
//! # User payloads
//!
//! The struct path for a user-declared reply/output is reconstructed from
//! its descriptor path (`<scope>::__ankyra_descriptor_<Name>` becomes
//! `<scope>::<Name>`), which relies on `#[klipper_reply]` /
//! `#[klipper_output]` emitting the descriptor fn next to the struct.

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::Ident;

use crate::identify::{
    IDENTIFY_RESPONSE_REPLY_ID, IDENTIFY_RESPONSE_REPLY_NAME, SHUTDOWN_REPLY_NAME,
};
use crate::sort::{AssembledItem, Assembly, ItemKind};

/// Emit the full sender module: struct definition, identify-response impl,
/// shutdown impl, and one impl per user-defined reply/output.
pub(crate) fn emit(assembly: &Assembly) -> TokenStream2 {
    let transport_struct = quote! {
        /// Firmware sender used by command dispatch to emit replies and
        /// outputs. Zero-sized — every call resolves through
        /// [`KLIPPER_TRANSPORT`]'s `encode_frame`.
        pub struct Sender;
    };

    let identify_id = IDENTIFY_RESPONSE_REPLY_ID;
    let identify_impl = quote! {
        impl ::ankyra::SendReply<IdentifyResponse> for Sender {
            fn send(&mut self, payload: IdentifyResponse) {
                KLIPPER_TRANSPORT.encode_frame(|buf| {
                    <u16 as ::ankyra::encoding::Writable>::write(&#identify_id, buf);
                    <IdentifyResponse as ::ankyra::encoding::Writable>::write(&payload, buf);
                });
            }
        }
    };

    let shutdown_id = shutdown_id(assembly);
    let shutdown_path = quote!(::ankyra::Shutdown);
    let shutdown_guard = frame_guard(&shutdown_path, shutdown_id, 0, "reply");
    let shutdown_impl = quote! {
        #shutdown_guard
        impl ::ankyra::SendReply<::ankyra::Shutdown> for Sender {
            fn send(&mut self, payload: ::ankyra::Shutdown) {
                KLIPPER_TRANSPORT.encode_frame(|buf| {
                    <u16 as ::ankyra::encoding::Writable>::write(&#shutdown_id, buf);
                    <::ankyra::Shutdown as ::ankyra::encoding::Writable>::write(&payload, buf);
                });
            }
        }
    };

    let mut user_impls = TokenStream2::new();
    for item in assembly.items() {
        match item.kind {
            ItemKind::Reply => {
                if item.name == IDENTIFY_RESPONSE_REPLY_NAME || item.name == SHUTDOWN_REPLY_NAME {
                    continue;
                }
                if let Some(struct_path) = struct_path_from_descriptor(item) {
                    user_impls.extend(emit_reply_impl(&struct_path, item.id, item.lifetime_count));
                }
            }
            ItemKind::Output => {
                if let Some(struct_path) = struct_path_from_descriptor(item) {
                    user_impls.extend(emit_output_impl(&struct_path, item.id, item.lifetime_count));
                }
            }
            ItemKind::Command => {}
        }
    }

    quote! {
        #transport_struct
        #identify_impl
        #shutdown_impl
        #user_impls
    }
}

/// Reconstruct the user struct path by rewriting the trailing
/// `__ankyra_descriptor_<Name>` segment of the item's descriptor path to
/// `<Name>`. Items without a parseable descriptor path get no sender impl.
fn struct_path_from_descriptor(item: &AssembledItem) -> Option<TokenStream2> {
    let desc_path = item.descriptor_path.as_ref()?;
    let mut path: syn::Path = syn::parse2(desc_path.clone()).ok()?;
    let last = path.segments.last_mut()?;
    let last_ident = last.ident.to_string();
    let struct_name = last_ident.strip_prefix("__ankyra_descriptor_")?;
    last.ident =
        syn::parse_str::<Ident>(struct_name).unwrap_or_else(|_| format_ident!("{}", struct_name));
    last.arguments = syn::PathArguments::None;
    Some(quote! { #path })
}

/// The sorted id assigned to the synthesized `shutdown` reply.
fn shutdown_id(assembly: &Assembly) -> u16 {
    assembly
        .items()
        .iter()
        .find(|i| i.kind == ItemKind::Reply && i.name == SHUTDOWN_REPLY_NAME)
        .expect("sort stage must synthesize the shutdown reply")
        .id
}

/// Emit one `SendReply<R>` impl for a user-declared reply struct.
///
/// `lifetime_count` is the number of lifetime generics declared on the
/// user struct, extracted from the `lt<N>` infix in the carrier ident
/// (see `input::split_name_with_lifetime_count`). When `N > 0`, the impl
/// header is synthesised with matching `'__ankyra_a0, …` generics so
/// `#[klipper_reply] pub struct TraceData<'a> { .. }` generates
/// `impl<'__ankyra_a0> SendReply<TraceData<'__ankyra_a0>> for Sender`, not
/// the rustc-rejected `impl SendReply<TraceData> for Sender` (E0726).
fn emit_reply_impl(struct_path: &TokenStream2, id: u16, lifetime_count: usize) -> TokenStream2 {
    let trait_ident = quote!(SendReply);
    let guard = frame_guard(struct_path, id, lifetime_count, "reply");
    let sender_impl = emit_sender_impl(struct_path, id, lifetime_count, &trait_ident);
    quote!(#guard #sender_impl)
}

/// Emit one `SendOutput<O>` impl for a user-declared output struct. See
/// [`emit_reply_impl`] for the lifetime-count rationale.
fn emit_output_impl(struct_path: &TokenStream2, id: u16, lifetime_count: usize) -> TokenStream2 {
    let trait_ident = quote!(SendOutput);
    let guard = frame_guard(struct_path, id, lifetime_count, "output");
    let sender_impl = emit_sender_impl(struct_path, id, lifetime_count, &trait_ident);
    quote!(#guard #sender_impl)
}

/// A const panic cannot format the computed size, so the message names only
/// the payload and its id; `<T as ::ankyra::ReplyWireSize>::MAX_PAYLOAD_BYTES`
/// holds the size.
fn frame_guard(
    struct_path: &TokenStream2,
    id: u16,
    lifetime_count: usize,
    kind: &str,
) -> TokenStream2 {
    let message = format!(
        "{kind} `{}` (id {id}) exceeds the {}-byte Klipper frame payload",
        render_path(struct_path),
        MESSAGE_PAYLOAD_MAX
    );
    let statics = (0..lifetime_count).map(|_| quote!('static));
    let ty = if lifetime_count == 0 {
        quote!(#struct_path)
    } else {
        quote!(#struct_path<#(#statics),*>)
    };
    quote! {
        const _: () = ::core::assert!(::ankyra::reply_fits::<#ty>(#id), #message);
    }
}

pub(crate) const MESSAGE_PAYLOAD_MAX: usize = 59;

fn render_path(path: &TokenStream2) -> String {
    let rendered = path.to_string().replace(' ', "");
    match rendered.strip_prefix("$crate::") {
        Some(rest) => rest.to_string(),
        None => rendered,
    }
}

fn emit_sender_impl(
    struct_path: &TokenStream2,
    id: u16,
    lifetime_count: usize,
    trait_ident: &TokenStream2,
) -> TokenStream2 {
    if lifetime_count == 0 {
        return quote! {
            impl ::ankyra::#trait_ident<#struct_path> for Sender {
                fn send(&mut self, payload: #struct_path) {
                    KLIPPER_TRANSPORT.encode_frame(|buf| {
                        <u16 as ::ankyra::encoding::Writable>::write(&#id, buf);
                        <#struct_path as ::ankyra::encoding::Writable>::write(&payload, buf);
                    });
                }
            }
        };
    }
    let lifetimes: Vec<syn::Lifetime> = (0..lifetime_count)
        .map(|i| syn::Lifetime::new(&format!("'__ankyra_a{i}"), proc_macro2::Span::call_site()))
        .collect();
    let lt_args = quote!(#(#lifetimes),*);
    quote! {
        impl<#lt_args> ::ankyra::#trait_ident<#struct_path<#lt_args>> for Sender {
            fn send(&mut self, payload: #struct_path<#lt_args>) {
                KLIPPER_TRANSPORT.encode_frame(|buf| {
                    <u16 as ::ankyra::encoding::Writable>::write(&#id, buf);
                    <#struct_path<#lt_args> as ::ankyra::encoding::Writable>::write(&payload, buf);
                });
            }
        }
    }
}
