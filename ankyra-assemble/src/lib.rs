//! `ankyra-assemble` — terminal proc-macro of ankyra's CPS-chain protocol
//! assembler.
//!
//! `ankyra_config!` folds every `ankyra_provider!` registration
//! into a single call to `__ankyra_assemble!`. This crate's job is to
//! take the collected carrier tuples, synthesize the reserved
//! `identify` / `identify_response` / `shutdown` items, canonicalize IDs,
//! assign static-string IDs, and re-emit a module tree that the firmware
//! crate can refer to via `KLIPPER_TRANSPORT` and
//! `_ankyra_config::static_strings::__ANKYRA_SS_<hash>`.
//!
//! # Rendezvous strategy
//!
//! Proc-macros do not expand declarative macros in their input token
//! stream, so `__ankyra_assemble!` receives unexpanded carrier calls
//! rather than the tuples the carrier macros would produce. The
//! rendezvous used here is **path reconstruction at input parse time**:
//!
//! * Every carrier macro is `#[macro_export]`, hoisted to its defining
//!   crate's root, with its `__ankyra_item_<kind>_<name>` ident encoding
//!   the item's kind and name.
//! * `crate::input::parse_carrier_call` pulls the last path segment to
//!   recover kind and name, then keeps the remaining prefix.
//! * Emission reconstructs the sibling paths at the same scope:
//!   `<prefix>::__ankyra_dispatch_<name>` for commands,
//!   `<prefix>::__ankyra_descriptor_<name>` for replies / outputs, and
//!   `<prefix>::<Name>` for the user struct type used in `SendReply` /
//!   `SendOutput` impls.
//!
//! This works in v0.1 because every `#[klipper_*]` attribute emits its
//! dispatch fn, descriptor fn, and carrier at the same module. The
//! same-crate case falls back to `crate::…` because `#[macro_export]`
//! publishes the carrier at the firmware crate's root.
//!
//! # v0.2 rendezvous notes
//!
//! * **No access to `message_format` strings** at assembler expansion
//!   time — the carrier macro has not expanded. The dictionary builder
//!   therefore uses the protocol name as a placeholder format for user
//!   commands/replies/outputs. The three synthesized items carry their
//!   Klipper-accurate formats because we own them here.
//! * **Module prefix comes from the provider wrapper.** v0.2 extends
//!   `ankyra_provider!`'s item lists to accept paths, which the
//!   companion macro emits as `{ prefix: (…), carrier!() }` tuples.
//!   `parse_wrapped_carrier_call` reads the prefix syntactically and
//!   threads it through `ItemInput`/`DefinitionInput::module_prefix`
//!   into sibling-path reconstruction. The v0.1 `parse_carrier_call`
//!   branch is preserved for bare-carrier entries (legacy v0.1
//!   provider output and synthetic inline items).
//!
mod dictionary;
mod dispatch;
mod identify;
mod input;
mod senders;
mod shared;
mod sort;
mod static_strings;

use proc_macro::TokenStream;
use proc_macro_error2::{abort, proc_macro_error};

/// Terminal `__ankyra_assemble!` entry point.
///
/// This function:
///
/// 1. Parses the `config = { .. }, items = [ .. ]` token stream fed in by
///    `ankyra_config!`.
/// 2. Runs `sort::assemble` to sort, dedup, assign IDs, and synthesize
///    `identify` / `identify_response` / `shutdown`.
/// 3. Emits the firmware-facing `_ankyra_config` module tree containing:
///    * `DICT_BYTES` — the Klipper data dictionary JSON bytes.
///    * `IdentifyResponse` + `handle_identify` — the bootstrap reply
///      struct and dispatch helper for the `identify` command.
///    * `Sender` + `SendReply` / `SendOutput` impls — the sender type
///      command handlers dispatch through. Always implements
///      `SendReply<IdentifyResponse>` (id 0) and
///      `SendReply<::ankyra::Shutdown>` (its sorted id); also covers
///      user-declared reply / output payloads whose struct paths can be
///      reconstructed from the carrier prefix.
///    * `Config` — implements `ankyra::transport::Config` with a
///      dispatch match on command id. Unknown ids return
///      `Err(ReadError)` to trip transport resync.
///    * `KLIPPER_TRANSPORT` — the single `Transport<Config>` instance
///      the firmware owns, threaded through `Sender` impls' `encode_frame`.
///    * `static_strings::__ANKYRA_SS_<hash>` — one `pub const u16` per
///      registered literal.
/// 4. Re-exports `KLIPPER_TRANSPORT` at the firmware crate root so
///    consumers can write `KLIPPER_TRANSPORT.receive(...)` without
///    qualifying the path.
#[doc(hidden)]
#[proc_macro]
#[proc_macro_error]
pub fn __ankyra_assemble(tokens: TokenStream) -> TokenStream {
    let parsed = match input::parse(tokens.into()) {
        Ok(p) => p,
        Err(e) => return e.to_compile_error().into(),
    };

    let assembly = match sort::assemble(parsed.items, parsed.static_strings) {
        Ok(a) => a,
        Err(e) => abort!(proc_macro2::Span::call_site(), "{}", e),
    };

    let transport_path = parsed.transport_path.unwrap_or_else(|| {
        abort!(
            proc_macro2::Span::call_site(),
            "ankyra_config! requires `transport = <path>: <type>`"
        )
    });
    let transport_ty = parsed.transport_ty.unwrap_or_else(|| {
        abort!(
            proc_macro2::Span::call_site(),
            "ankyra_config! requires `transport = <path>: <type>`"
        )
    });
    let context_ty = parsed.context_ty.unwrap_or_else(|| {
        abort!(
            proc_macro2::Span::call_site(),
            "ankyra_config! requires `context = <type>`"
        )
    });

    // Constants + enumerations are threaded into the dictionary's
    // `config` and `enumerations` sections via the same carrier-arm
    // dispatch as replies/outputs (see `dictionary::emit`). The four
    // trailer metadata overrides (`app`, `version`, `build_versions`,
    // `license`) ride alongside — `None` means "apply ankyra's default"
    // so existing consumers see no wire change.
    let metadata = dictionary::TrailerMetadata {
        app: parsed.app,
        version: parsed.version,
        build_versions: parsed.build_versions,
        license: parsed.license,
    };
    let dict_bytes = dictionary::emit(&assembly, &parsed.definitions, &metadata);
    let identify_mod = identify::emit();
    let sender_mod = senders::emit(&assembly);
    let config_mod = dispatch::emit(&assembly, &transport_ty, &context_ty);
    let ss_consts = static_strings::emit(assembly.static_strings());

    // The transport binding is intentionally emitted at the firmware crate
    // root (via the `pub(crate) use` re-export below) rather than inside
    // the `_ankyra_config` submodule. This matches the firmware ergonomics
    // spec — users write `KLIPPER_TRANSPORT.receive(...)` without a
    // module qualifier.
    quote::quote! {
        #[doc(hidden)]
        #[allow(non_snake_case, non_camel_case_types)]
        pub mod _ankyra_config {
            use super::*;

            #dict_bytes

            #identify_mod

            #sender_mod

            #config_mod

            /// The firmware's single `Transport<Config>` instance. All
            /// inbound bytes flow through `KLIPPER_TRANSPORT.receive(...)`
            /// and all outbound frames flow through the `Sender` impls'
            /// `encode_frame` closures.
            pub static KLIPPER_TRANSPORT: ::ankyra::transport::Transport<Config> =
                ::ankyra::transport::Transport::<Config>::new(
                    &Config,
                    #transport_path,
                );

            pub mod static_strings {
                #ss_consts
            }
        }

        pub(crate) use self::_ankyra_config::KLIPPER_TRANSPORT;
    }
    .into()
}
