//! Emit the `pub mod static_strings { ... }` submodule.
//!
//! The firmware references static strings via
//! `crate::_ankyra_config::static_strings::__ANKYRA_SS_<hash>`, a path
//! constructed by the `ankyra_macros::klipper_static_string` and
//! `ankyra_macros::klipper_shutdown` macros. This module emits the
//! matching `pub const __ANKYRA_SS_<hash>: u16 = <id>;` for every literal
//! the firmware listed in `ankyra_config! { static_strings = [...] }`.
//!
//! IDs come from `crate::sort::Assembly::static_strings` which assigns
//! them starting at 2 (ids 0 and 1 are reserved by Klipper for protocol
//! internals). Each const is named by the same `ankyra_codegen::fnv1a_64`
//! hash that `klipper_static_string!` uses.

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};

use ankyra_codegen::fnv1a_64;

/// Emit the static-strings module body.
///
/// The caller splices the returned tokens into a `pub mod static_strings { … }`
/// block inside the `_ankyra_config` module tree.
pub(crate) fn emit(static_strings: &[(String, u16)]) -> TokenStream2 {
    let consts = static_strings.iter().map(|(content, id)| {
        let hash = fnv1a_64(content.as_bytes());
        let ident = format_ident!("__ANKYRA_SS_{:016x}", hash);
        let id = *id;
        quote! { pub const #ident: u16 = #id; }
    });
    quote! {
        #(#consts)*
    }
}
