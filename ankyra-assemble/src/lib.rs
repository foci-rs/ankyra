//! `ankyra-assemble` — terminal proc-macro of ankyra's CPS-chain protocol
//! assembler.
//!
//! `ankyra_config!` (Task 11) folds every `ankyra_provider!` registration
//! into a single call to [`__ankyra_assemble`]. This crate's job is to
//! take the collected carrier tuples, synthesize the reserved
//! `identify` / `identify_response` / `shutdown` items, canonicalize IDs,
//! assign static-string IDs, and re-emit a module tree that the firmware
//! crate can refer to via `KLIPPER_TRANSPORT` and
//! `_ankyra_config::static_strings::__ANKYRA_SS_<hash>`.
//!
//! Task 10 (this slice) implements the sort/dedup/identify stage plus a
//! minimal stub emission that is just rich enough for Task 11's CPS fold
//! to compile end-to-end. The full dictionary JSON / dispatch match arms
//! / sender impls / transport binding are Task 12's responsibility.

mod identify;
mod input;
mod shared;
mod sort;

use proc_macro::TokenStream;
use proc_macro_error2::{abort, proc_macro_error};

/// Stub `__ankyra_assemble!` entry point — Task 10 scope.
///
/// See the module docs for the full pipeline. This function:
///
/// 1. Parses the `config = { .. }, items = [ .. ]` token stream fed in by
///    `ankyra_config!`.
/// 2. Runs `sort::assemble` to sort, dedup, assign IDs, and synthesize
///    `identify` / `identify_response` / `shutdown`.
/// 3. Emits a minimal module tree the firmware crate can observe:
///    `mod _ankyra_config { pub(crate) const TRANSPORT: () = (); pub mod
///    static_strings { pub const __ANKYRA_SS_<hash>: u16 = <id>; } }` plus
///    a `pub(crate) use self::_ankyra_config::TRANSPORT as
///    KLIPPER_TRANSPORT` re-export at the firmware crate root.
///
/// Task 12 will grow this to emit the full dispatcher, sender impls,
/// compressed data dictionary, and a real `KLIPPER_TRANSPORT` binding.
#[doc(hidden)]
#[proc_macro]
#[proc_macro_error]
pub fn __ankyra_assemble(tokens: TokenStream) -> TokenStream {
    let parsed = match input::parse(tokens.into()) {
        Ok(p) => p,
        Err(e) => return e.to_compile_error().into(),
    };

    // Definitions (constants + enumerations) flow into Task 12's data
    // dictionary emitter. Task 10 intentionally drops them on the floor
    // but binds them to `_` so clippy does not flag the unused field —
    // and so a reviewer sees the intentional ignore rather than a silent
    // omission.
    let _ = &parsed.definitions;
    let _ = &parsed.transport_path;
    let _ = &parsed.transport_ty;
    let _ = &parsed.context_ty;

    let assembly = match sort::assemble(parsed.items, parsed.static_strings) {
        Ok(a) => a,
        Err(e) => abort!(proc_macro2::Span::call_site(), "{}", e),
    };

    let ss_consts = assembly.static_strings().iter().map(|(content, id)| {
        let hash = shared::fnv1a_64(content.as_bytes());
        let ident = quote::format_ident!("__ANKYRA_SS_{:016x}", hash);
        let id = *id;
        quote::quote! { pub const #ident: u16 = #id; }
    });

    quote::quote! {
        #[doc(hidden)]
        mod _ankyra_config {
            pub(crate) const TRANSPORT: () = ();
            pub mod static_strings {
                #(#ss_consts)*
            }
        }
        pub(crate) use self::_ankyra_config::TRANSPORT as KLIPPER_TRANSPORT;
    }
    .into()
}
