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
//! Three synthesized payloads always get impls regardless of user input:
//!
//! * `IdentifyResponse` (id 0) — the reply to the bootstrap `identify`
//!   command.
//! * `ankyra::Shutdown` at its assigned id — the firmware-wide unrecoverable
//!   fault signal.
//!
//! # v0.1 scope note
//!
//! User-declared reply/output payloads are surfaced via carrier macros
//! that reference `$crate::<Name>` structs emitted by `#[klipper_reply]` /
//! `#[klipper_output]`. The assembler reconstructs that path by pulling
//! the carrier path prefix (see `crate::input::parse_carrier_call`) and
//! appending `<Name>`. Emitted `SendReply` / `SendOutput` impls therefore
//! work as long as the user-level struct shares its module with its
//! carrier macro — the v0.1 invariant documented on the item-level
//! macros. Task 13's cross-crate example exercises this at a real
//! extern-crate boundary.

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::Ident;

use crate::identify::{
    IDENTIFY_RESPONSE_REPLY_ID, IDENTIFY_RESPONSE_REPLY_NAME, SHUTDOWN_REPLY_NAME,
};
use crate::sort::{AssembledItem, Assembly};

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
    let shutdown_impl = quote! {
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
            "reply" => {
                if item.name == IDENTIFY_RESPONSE_REPLY_NAME || item.name == SHUTDOWN_REPLY_NAME {
                    continue;
                }
                if let Some(struct_path) = struct_path_from_descriptor(item) {
                    user_impls.extend(emit_reply_impl(&struct_path, item.id, item.lifetime_count));
                }
            }
            "output" => {
                if let Some(struct_path) = struct_path_from_descriptor(item) {
                    user_impls.extend(emit_output_impl(&struct_path, item.id, item.lifetime_count));
                }
            }
            _ => {}
        }
    }

    quote! {
        #transport_struct
        #identify_impl
        #shutdown_impl
        #user_impls
    }
}

/// Reconstruct the user struct path from the item's descriptor path.
///
/// `input::parse_carrier_call` stored the descriptor fn path for reply/
/// output items as `<prefix>::__ankyra_descriptor_<Name>`. Swapping the
/// last segment for just `<Name>` yields the struct type path — v0.1
/// requires `#[klipper_reply]` / `#[klipper_output]` to live at the same
/// scope as their carrier macro, which is also where the descriptor fn
/// and struct live, so this path resolves.
///
/// Returns `None` when the descriptor path is missing (the carrier did not
/// round-trip a path — should not happen in practice because the input
/// parser always reconstructs one). No diagnostic is emitted; the item
/// simply gets no sender impl, and a later compile error on a
/// `klipper_reply!` call-site will surface the gap.
fn struct_path_from_descriptor(item: &AssembledItem) -> Option<TokenStream2> {
    let desc_path = item.descriptor_path.as_ref()?;
    let parsed: syn::Path = syn::parse2(desc_path.clone()).ok()?;
    let mut new_path = syn::Path {
        leading_colon: parsed.leading_colon,
        segments: syn::punctuated::Punctuated::default(),
    };
    let segs: Vec<_> = parsed.segments.iter().cloned().collect();
    if segs.is_empty() {
        return None;
    }
    let last_idx = segs.len() - 1;
    for (i, seg) in segs.iter().enumerate() {
        if i < last_idx {
            new_path.segments.push(seg.clone());
        }
    }
    // Replace the trailing `__ankyra_descriptor_<Name>` segment with
    // `<Name>` alone.
    let last_seg = &segs[last_idx];
    let last_ident = last_seg.ident.to_string();
    let struct_name = last_ident.strip_prefix("__ankyra_descriptor_")?;
    let struct_ident: Ident =
        syn::parse_str(struct_name).unwrap_or_else(|_| format_ident!("{}", struct_name));
    new_path.segments.push(syn::PathSegment {
        ident: struct_ident,
        arguments: syn::PathArguments::None,
    });
    Some(quote! { #new_path })
}

/// Compute the sorted id assigned to the `shutdown` reply. The sort stage
/// guarantees one exists; panicking here would mean the assembler
/// invariant has drifted from `identify.rs`.
fn shutdown_id(assembly: &Assembly) -> u16 {
    assembly
        .items()
        .iter()
        .find(|i| i.kind == "reply" && i.name == SHUTDOWN_REPLY_NAME)
        .expect("sort stage must synthesize the shutdown reply")
        .id
}

/// Emit one `SendReply<R>` impl for a user-declared reply struct.
///
/// `lifetime_count` is the number of lifetime generics declared on the
/// user struct, extracted from the `lt<N>` infix in the carrier ident
/// (see `input::split_name_with_lifetime_count`). When `N > 0`, the impl
/// header is synthesised with matching `'__a0, '__a1, …` generics so
/// `#[klipper_reply] pub struct FociTraceData<'a> { .. }` generates
/// `impl<'__a0> SendReply<FociTraceData<'__a0>> for Sender`, not the
/// rustc-rejected `impl SendReply<FociTraceData> for Sender` (E0726).
fn emit_reply_impl(struct_path: &TokenStream2, id: u16, lifetime_count: usize) -> TokenStream2 {
    let trait_ident = quote!(SendReply);
    emit_sender_impl(struct_path, id, lifetime_count, &trait_ident)
}

/// Emit one `SendOutput<O>` impl for a user-declared output struct. See
/// [`emit_reply_impl`] for the lifetime-count rationale.
fn emit_output_impl(struct_path: &TokenStream2, id: u16, lifetime_count: usize) -> TokenStream2 {
    let trait_ident = quote!(SendOutput);
    emit_sender_impl(struct_path, id, lifetime_count, &trait_ident)
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
    let lt_header = quote!(#(#lifetimes),*);
    let lt_args = quote!(#(#lifetimes),*);
    quote! {
        impl<#lt_header> ::ankyra::#trait_ident<#struct_path<#lt_args>> for Sender {
            fn send(&mut self, payload: #struct_path<#lt_args>) {
                KLIPPER_TRANSPORT.encode_frame(|buf| {
                    <u16 as ::ankyra::encoding::Writable>::write(&#id, buf);
                    <#struct_path<#lt_args> as ::ankyra::encoding::Writable>::write(&payload, buf);
                });
            }
        }
    }
}
