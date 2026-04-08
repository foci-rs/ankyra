//! Emit the `pub mod static_strings { ... }` submodule.
//!
//! The firmware references static strings via
//! `crate::_ankyra_config::static_strings::__ANKYRA_SS_<hash>`, a path
//! constructed by [`ankyra_macros::klipper_static_string`] and
//! [`ankyra_macros::klipper_shutdown`]. This module emits the matching
//! `pub const __ANKYRA_SS_<hash>: u16 = <id>;` for every literal the
//! firmware listed in `ankyra_config! { static_strings = [...] }`.
//!
//! IDs come from [`crate::sort::Assembly::static_strings`] which assigns
//! them starting at 2 (ids 0 and 1 are reserved by Klipper for protocol
//! internals). The FNV-1a hash used to name each const matches the hash
//! used by `klipper_static_string!` — both sides read through
//! [`crate::shared::fnv1a_64`] / [`ankyra_macros::shared::fnv1a_64`], and
//! the two copies are tied together by a fixed-value probe test in each
//! crate's `shared` module.

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};

use crate::shared::fnv1a_64;

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
