//! Klipper MCU protocol framework for Rust. `no_std`-compatible, proc-macro
//! driven, composable across crates.
//!
//! ankyra lets library crates publish self-contained protocol *providers*
//! — bundles of commands, replies, outputs, constants, and enumerations —
//! which a firmware binary aggregates into a single `Transport` at compile
//! time. The runtime exposes the wire-format encoding, framing, and dispatch
//! primitives; the [`ankyra-macros`](https://docs.rs/ankyra-macros) crate
//! supplies the [`#[klipper_command]`][`klipper_command`],
//! [`#[klipper_reply]`][`klipper_reply`], [`#[klipper_output]`][`klipper_output`],
//! [`#[klipper_constant]`][`klipper_constant`], [`klipper_enumeration!`],
//! [`ankyra_provider!`], and [`ankyra_config!`] macros that drive it.
//!
//! # Firmware quickstart
//!
//! Library crate exposing one provider:
//!
//! ```ignore
//! use ankyra::prelude::*;
//!
//! pub trait ClockView {
//!     fn now(&self) -> u32;
//! }
//!
//! #[klipper_reply]
//! pub struct ClockReply {
//!     pub clock: u32,
//! }
//!
//! #[klipper_command]
//! fn get_clock(ctx: &mut dyn ClockView) {
//!     ::ankyra::klipper_reply!(ClockReply, clock: u32 = ctx.now());
//! }
//!
//! ankyra_provider! {
//!     name: CLOCK_PROVIDER,
//!     commands: [get_clock],
//!     replies: [ClockReply],
//! }
//! ```
//!
//! Firmware crate aggregating it:
//!
//! ```ignore
//! use ankyra::prelude::*;
//!
//! pub struct MyTransportOutput;
//! pub const TRANSPORT_OUTPUT: MyTransportOutput = MyTransportOutput;
//!
//! impl ankyra::TransportOutput for MyTransportOutput {
//!     type Output = ankyra::ScratchOutput<128>;
//!     fn output(&self, f: impl FnOnce(&mut Self::Output)) {
//!         let mut o = ankyra::ScratchOutput::<128>::new();
//!         f(&mut o);
//!         // ship bytes to USB/UART here
//!     }
//! }
//!
//! ankyra_config! {
//!     transport = crate::TRANSPORT_OUTPUT: crate::MyTransportOutput,
//!     context = &'ctx mut MyState,
//!     providers = [my_library::CLOCK_PROVIDER],
//!     static_strings = [],
//! }
//! ```
//!
//! For a complete runnable cross-crate example see
//! [`examples/clock_lib`](https://github.com/mjonuschat/ankyra/tree/main/examples/clock_lib)
//! and
//! [`examples/clock_firmware`](https://github.com/mjonuschat/ankyra/tree/main/examples/clock_firmware)
//! in the repository.
//!
//! # Cross-crate model
//!
//! * **Library crates** declare protocol items with the [`#[klipper_command]`][`klipper_command`]
//!   / [`#[klipper_reply]`][`klipper_reply`] / [`#[klipper_output]`][`klipper_output`] /
//!   [`#[klipper_constant]`][`klipper_constant`] attributes and [`klipper_enumeration!`] /
//!   [`klipper_static_string!`] macros at the crate root, then expose them with a
//!   single [`ankyra_provider!`] registration.
//! * **Firmware crates** pick the providers they want via
//!   `ankyra_config! { providers = [other_crate::PROVIDER, ...], ... }`. The
//!   assembler folds every selected provider's items into one data dictionary,
//!   one dispatch table, and one sender type.
//! * **The compiler enforces closure.** If a firmware aggregates a command
//!   that sends a reply type for which the firmware's sender has no matching
//!   `impl SendReply<R> for Sender`, the build fails with E0277. Forgetting to
//!   include a reply type is a type error, not a silent runtime fault.
//!
//! # Mental model
//!
//! [`ankyra_config!`] drives a compile-time fold over the listed providers,
//! collecting their items into a single data dictionary, dispatch table, and
//! sender struct. Each provider contributes its commands, replies, outputs,
//! constants, and enumerations; the assembler resolves IDs, stitches in user
//! format strings, and emits the [`Transport<Config>`][`Transport`] value
//! your RTIC (or Embassy, or bare-metal) executor instantiates at startup.
//! Everything is built by source-level macro expansion — no `build.rs`
//! source-walking, no runtime reflection.
//!
//! # v0.1 limitations
//!
//! * [`#[klipper_command]`][`klipper_command`], [`#[klipper_reply]`][`klipper_reply`],
//!   [`#[klipper_output]`][`klipper_output`], [`#[klipper_constant]`][`klipper_constant`],
//!   [`klipper_enumeration!`], and [`klipper_static_string!`] items must live
//!   at the defining crate's root (not in submodules). The individual macro
//!   rustdoc details the underlying carrier-visibility rule.
//! * Dictionary compression uses RFC 1950 stored blocks (valid zlib framing,
//!   no actual deflate). Klipper host interop works today; flash and
//!   throughput savings from real deflate are a v0.2 follow-up.
//! * Constants and enumerations ship in the dictionary but are not yet
//!   exposed as a runtime query surface.
//!
//! # Platform neutrality
//!
//! ankyra has no MCU, board, or executor assumptions; implement
//! [`TransportOutput`] against whichever USB/UART driver your target uses
//! and drive the resulting [`Transport`] from RTIC, Embassy, an interrupt
//! handler, or a plain `main` loop.
//!
//! [`Transport`]: crate::transport::Transport
//! [`TransportOutput`]: crate::transport_output::TransportOutput
//! [`klipper_command`]: macro@crate::klipper_command
//! [`klipper_reply`]: macro@crate::klipper_reply
//! [`klipper_output`]: macro@crate::klipper_output
//! [`klipper_constant`]: macro@crate::klipper_constant
//! [`klipper_enumeration!`]: macro@crate::klipper_enumeration
//! [`klipper_static_string!`]: macro@crate::klipper_static_string
//! [`ankyra_provider!`]: macro@crate::ankyra_provider
//! [`ankyra_config!`]: macro@crate::ankyra_config

#![cfg_attr(not(feature = "std"), no_std)]

/// Re-export of the `const_format` crate.
///
/// The assembler emits the firmware's data dictionary JSON as a
/// `::ankyra::const_format::concatcp!` invocation so that user-item format
/// strings and constant/enumeration values — which the assembler cannot
/// access at proc-macro time because their carrier `macro_rules!` have not
/// expanded yet — are stitched into the dictionary string at const-eval
/// time. Re-exporting through the `ankyra` crate means firmware crates do
/// not need to list `const_format` as a direct dependency.
#[doc(hidden)]
pub use const_format;

pub mod descriptor;
pub mod dictionary;
pub mod encoding;
mod fifo_buffer;
mod input_buffer;
mod output_buffer;
pub mod provider;
pub mod reply;
pub mod send;
pub mod shutdown;
pub mod transport;
pub mod transport_output;

pub use fifo_buffer::FifoBuffer;
pub use input_buffer::{InputBuffer, SliceInputBuffer};
pub use output_buffer::{OutputBuffer, ScratchOutput};
pub use send::{SendOutput, SendReply};
pub use shutdown::Shutdown;
pub use transport::{ShutdownState, Transport, oversize_frame_drops};
pub use transport_output::TransportOutput;

/// Emit a reply from inside a `#[klipper_command]` handler body.
///
/// Shape: `klipper_reply!(R, field1 [: ty] = expr, field2 [: ty] = expr, ...)`.
///
/// Expands to `<_ as ::ankyra::SendReply<R>>::send(__ankyra_sender, R { ... })`.
/// The turbofish on `R` forces the `SendReply<R>` trait; the `_` placeholder
/// lets the compiler infer the concrete sender type from the
/// `__ankyra_sender` binding introduced by the `#[klipper_command]` dispatch
/// wrapper. Therefore this macro only typechecks inside a handler body.
///
/// The optional `: ty` annotation on each field is purely documentary; it
/// is parsed but not spliced into the emitted struct literal. The field's
/// declared type on the reply struct governs the actual value's type.
///
/// # Why a `pub use` re-export
///
/// Rust's proc-macro system reserves a single macro namespace per ident, so
/// an attribute `#[klipper_reply]` and a fn-like `klipper_reply!` cannot
/// both live in `ankyra-macros` at the same name. The fn-like proc-macro
/// is published there under the internal name `__klipper_reply_call_site`
/// and re-exported here as `klipper_reply`. Using a proc-macro instead of a
/// `macro_rules!` keeps the emitted `__ankyra_sender` ident tagged with
/// `Span::call_site()` hygiene, which is what lets it resolve against the
/// binding introduced by `#[klipper_command]`; a `macro_rules!` would
/// resolve the bare ident at the macro's definition site instead.
pub use ankyra_macros::__klipper_reply_call_site as klipper_reply;

/// Emit a reply from a non-handler context by naming the sender explicitly.
///
/// Shape: `klipper_reply_from!(sender_expr, R, field1 [: ty] = expr, ...)`.
///
/// Expands to (essentially)
///
/// ```ignore
/// {
///     let __ankyra_sender = <sender_expr>;
///     <_ as ::ankyra::SendReply<R>>::send(__ankyra_sender, R { ... })
/// }
/// ```
///
/// This form exists for emitters that cannot see the `__ankyra_sender`
/// binding `#[klipper_command]` injects — RTIC timer tasks, NVIC
/// interrupt handlers, spawned futures, and any other non-handler code
/// path that needs to push a frame to the host. Inside a handler body
/// the shorter [`klipper_reply!`] is still preferred; this macro is the
/// explicit-transport escape hatch.
///
/// The sender argument is anything that implements
/// `::ankyra::SendReply<R>` for the chosen reply type — typically
/// `&mut crate::_ankyra_config::Sender`, which the assembler emits with
/// matching impls for every reply declared across the firmware's
/// providers.
///
/// The sender expression is evaluated exactly once, even if the caller
/// passes a side-effectful expression (a function call, a `&mut` borrow
/// that produces a guard, etc.). The macro binds it to a local before
/// invoking `SendReply::send`.
///
/// The optional `: ty` annotation on each field is purely documentary; it
/// is parsed but not spliced into the emitted struct literal. The field's
/// declared type on the reply struct governs the actual value's type.
///
/// # Why a `pub use` re-export
///
/// Same reason [`klipper_reply!`] uses one: a proc-macro is required to
/// emit identifiers tagged with `Span::call_site()` hygiene (a
/// `macro_rules!` would resolve `__ankyra_sender` at its own definition
/// site instead of the caller's scope). The underlying macro lives in
/// `ankyra-macros` under an internal name and is re-exported here so
/// users reach for `::ankyra::klipper_reply_from!(...)`.
pub use ankyra_macros::__klipper_reply_from_call_site as klipper_reply_from;

/// Emit an output from inside a `#[klipper_command]` handler body.
///
/// Shape: `klipper_output!(O, field1 [: ty] = expr, field2 [: ty] = expr, ...)`.
///
/// Expands to `<_ as ::ankyra::SendOutput<O>>::send(__ankyra_sender, O { ... })`.
/// The turbofish on `O` forces the `SendOutput<O>` trait; the `_` placeholder
/// lets the compiler infer the concrete sender type from the
/// `__ankyra_sender` binding introduced by the `#[klipper_command]` dispatch
/// wrapper. Therefore this macro only typechecks inside a handler body.
///
/// The optional `: ty` annotation on each field is purely documentary; it
/// is parsed but not spliced into the emitted struct literal. The field's
/// declared type on the output struct governs the actual value's type.
///
/// # Why a `pub use` re-export
///
/// Rust's proc-macro system reserves a single macro namespace per ident, so
/// an attribute `#[klipper_output]` and a fn-like `klipper_output!` cannot
/// both live in `ankyra-macros` at the same name. The fn-like proc-macro
/// is published there under the internal name `__klipper_output_call_site`
/// and re-exported here as `klipper_output`. Using a proc-macro instead of
/// a `macro_rules!` keeps the emitted `__ankyra_sender` ident tagged with
/// `Span::call_site()` hygiene, which is what lets it resolve against the
/// binding introduced by `#[klipper_command]`; a `macro_rules!` would
/// resolve the bare ident at the macro's definition site instead.
pub use ankyra_macros::__klipper_output_call_site as klipper_output;

/// Emit an output from a non-handler context by naming the sender explicitly.
///
/// Shape: `klipper_output_from!(sender_expr, O, field1 [: ty] = expr, ...)`.
///
/// Expands to (essentially)
///
/// ```ignore
/// {
///     let __ankyra_sender = <sender_expr>;
///     <_ as ::ankyra::SendOutput<O>>::send(__ankyra_sender, O { ... })
/// }
/// ```
///
/// Mirror of [`klipper_reply_from!`] for unsolicited outputs — heartbeat
/// stats, trace samples, and other periodic frames emitted from RTIC
/// timer tasks, NVIC interrupt handlers, or any other non-handler code
/// path. Inside a handler body the shorter [`klipper_output!`] is still
/// preferred; this macro is the explicit-transport escape hatch.
///
/// The sender argument is anything that implements
/// `::ankyra::SendOutput<O>` for the chosen output type — typically
/// `&mut crate::_ankyra_config::Sender`.
///
/// The sender expression is evaluated exactly once. The optional `: ty`
/// annotation on each field is purely documentary; it is parsed but not
/// spliced into the emitted struct literal.
///
/// # Why a `pub use` re-export
///
/// Same reason as [`klipper_output!`]: the underlying proc-macro lives
/// in `ankyra-macros` under an internal name and is re-exported here so
/// users reach for `::ankyra::klipper_output_from!(...)`.
pub use ankyra_macros::__klipper_output_from_call_site as klipper_output_from;

/// Reference a registered static string by its message literal.
///
/// Shape: `klipper_static_string!("message")`.
///
/// Expands to `crate::_ankyra_config::static_strings::__ANKYRA_SS_<hash>`
/// where `<hash>` is the FNV-1a 64-bit digest of the literal's UTF-8 bytes.
/// The assembler (Task 12) emits the matching constant in that module when
/// the literal is listed in the firmware's `ankyra_config! { static_strings
/// = [...] }` entry. A literal not listed there causes the firmware build
/// to fail with `cannot find __ANKYRA_SS_<hash> in module static_strings` —
/// a distinct diagnostic from "silently mis-registered".
///
/// # Why a `pub use` re-export
///
/// Proc-macro crates ship only procedural macros; user-facing ergonomics
/// benefit from a stable import path rooted in the ankyra crate. The
/// underlying proc-macro lives in `ankyra-macros` and is re-exported here
/// so users write `::ankyra::klipper_static_string!("...")` alongside the
/// other call-site macros.
pub use ankyra_macros::klipper_static_string;

/// Emit a shutdown reply from inside a `#[klipper_command]` handler body.
///
/// Shape: `klipper_shutdown!("reason-string-literal", clock_expr)`.
///
/// Expands to
///
/// ```ignore
/// <_ as ::ankyra::SendReply<::ankyra::Shutdown>>::send(
///     __ankyra_sender,
///     ::ankyra::Shutdown {
///         clock: <clock_expr>,
///         static_string_id: crate::_ankyra_config::static_strings::__ANKYRA_SS_<hash>,
///     },
/// )
/// ```
///
/// The reason string literal is hashed with FNV-1a; the firmware build
/// fails unless the literal is listed in `ankyra_config! { static_strings =
/// [...] }`. Task 5's body-scan sees the `klipper_shutdown!` invocation
/// and folds an `S: SendReply<Shutdown>` bound onto the dispatch wrapper's
/// generics automatically.
///
/// # Why a `pub use` re-export
///
/// Same reason as `klipper_static_string!`: keep the user-facing call-site
/// macros in a single ankyra-rooted namespace regardless of which crate
/// actually defines them.
pub use ankyra_macros::klipper_shutdown;

/// Emit a shutdown reply from any context, including outside a
/// `#[klipper_command]` handler body.
///
/// Shape: `klipper_shutdown_from!(sender_expr, "reason-string-literal", clock_expr)`.
///
/// Expands to
///
/// ```ignore
/// {
///     let __ankyra_sender = <sender_expr>;
///     <_ as ::ankyra::SendReply<::ankyra::Shutdown>>::send(
///         __ankyra_sender,
///         ::ankyra::Shutdown {
///             clock: <clock_expr>,
///             static_string_id: crate::_ankyra_config::static_strings::__ANKYRA_SS_<hash>,
///         },
///     )
/// }
/// ```
///
/// Use this form when the shutdown must be raised from an RTIC task, a
/// timer callback, an NVIC interrupt handler, or any other non-handler
/// code path — anywhere the implicit `__ankyra_sender` parameter that
/// [`klipper_shutdown!`] relies on is not in scope. Inside a handler body
/// the shorter [`klipper_shutdown!`] is still preferred; this macro is the
/// explicit-transport escape hatch.
///
/// The sender argument is anything that implements
/// `::ankyra::SendReply<::ankyra::Shutdown>` — typically
/// `&mut crate::_ankyra_config::Sender`.
///
/// The sender expression is evaluated exactly once. The reason string
/// literal is FNV-1a-hashed and must still be listed in
/// `ankyra_config! { static_strings = [...] }`, exactly as with the
/// handler-scoped [`klipper_shutdown!`].
///
/// # Why a `pub use` re-export
///
/// Same reason as [`klipper_reply_from!`] / [`klipper_output_from!`]: the
/// underlying proc-macro lives in `ankyra-macros` under an internal name
/// (`__klipper_shutdown_from_call_site`) and is re-exported here so users
/// reach for `::ankyra::klipper_shutdown_from!(...)`.
pub use ankyra_macros::__klipper_shutdown_from_call_site as klipper_shutdown_from;

/// Build the firmware's protocol assembly from a list of provider registrations.
///
/// See the `ankyra_config!` documentation on `ankyra_macros` for the full
/// shape. Re-exported here so users refer to it by the stable path
/// `::ankyra::ankyra_config!`.
pub use ankyra_macros::ankyra_config;

/// `#[klipper_command]` attribute macro. Re-exported so library and
/// firmware crates can attach command handlers using the stable ankyra path.
pub use ankyra_macros::klipper_command;

/// `#[klipper_constant]` attribute macro. Re-exported so crates can publish
/// firmware constants via the stable ankyra path.
pub use ankyra_macros::klipper_constant;

/// `klipper_enumeration!` fn-like macro. Re-exported so crates can declare
/// enumerations via the stable ankyra path.
pub use ankyra_macros::klipper_enumeration;

/// `ankyra_provider!` fn-like macro. Re-exported so provider-defining
/// crates can register their items under a single ankyra-rooted namespace.
pub use ankyra_macros::ankyra_provider;

/// `ankyra_reexport_provider!` fn-like macro. Re-exported so crates can
/// re-expose another crate's provider through their own root using a
/// stable ankyra path.
pub use ankyra_macros::ankyra_reexport_provider;

/// Terminal assembler invoked by [`__ankyra_fold_providers!`].
///
/// Re-exported from `ankyra-assemble` so the fold continuation can refer to
/// it via `$crate::__ankyra_assemble!` and resolve against the `ankyra`
/// crate without forcing every consumer to depend on `ankyra-assemble`
/// directly.
#[doc(hidden)]
pub use ankyra_assemble::__ankyra_assemble;

/// CPS-fold continuation driver used by `ankyra_config!`.
///
/// `ankyra_config!` expands to a single invocation of this macro with
/// `config = { ... }`, an empty `accumulator`, and the list of provider
/// companion macros in `remaining`. Each recursive step hands control to
/// the first companion macro in `remaining`; that companion appends its
/// carrier tuples to `accumulator` and tail-calls back here with its entry
/// removed from `remaining`. When `remaining` is empty the accumulated
/// items are handed to `::ankyra_assemble::__ankyra_assemble!`, which is
/// where the dispatch table, sender impls, data dictionary, and transport
/// binding are synthesized.
///
/// # Why this macro lives in the `ankyra` runtime crate
///
/// Proc-macro crates (`ankyra-macros`) cannot export `macro_rules!` macros
/// — rustc rejects `#[macro_export]` on a declarative macro inside a crate
/// with `proc-macro = true`. The fold has to be a `macro_rules!` because it
/// performs token-level CPS recursion, not a single-shot transformation.
/// Hosting it here makes it reachable as `::ankyra::__ankyra_fold_providers!`
/// from tokens emitted by `ankyra_config!` and the per-provider companion
/// macros.
#[doc(hidden)]
#[macro_export]
macro_rules! __ankyra_fold_providers {
    (
        config = { $($cfg:tt)* },
        accumulator = [ $($acc:tt)* ],
        remaining = [ $first:path $(, $rest:path)* $(,)? ],
    ) => {
        $first ! {
            config = { $($cfg)* },
            accumulator = [ $($acc)* ],
            remaining = [ $($rest),* ],
        }
    };
    (
        config = { $($cfg:tt)* },
        accumulator = [ $($acc:tt)* ],
        remaining = [],
    ) => {
        $crate::__ankyra_assemble! {
            config = { $($cfg)* },
            items = [ $($acc)* ],
        }
    };
}

/// Convenience re-exports for end users.
///
/// Importing `ankyra::prelude::*` pulls in:
///
/// * Descriptor types ([`DefinitionDescriptor`], [`MessageDescriptor`], …).
/// * Provider types ([`ProviderRef`], [`ProviderSpec`]).
/// * Payload traits ([`OutputPayload`], [`ReplyPayload`]).
/// * Sender traits ([`SendOutput`], [`SendReply`]).
/// * The firmware-wide [`Shutdown`] reply type.
/// * Every ankyra attribute and function-like macro users reach for when
///   declaring commands, replies, outputs, constants, enumerations, and
///   providers. Bringing the macros into scope via the prelude matches the
///   ergonomics users expect from attribute-heavy DSLs.
///
/// [`DefinitionDescriptor`]: crate::descriptor::DefinitionDescriptor
/// [`MessageDescriptor`]: crate::descriptor::MessageDescriptor
/// [`ProviderRef`]: crate::provider::ProviderRef
/// [`ProviderSpec`]: crate::provider::ProviderSpec
/// [`OutputPayload`]: crate::reply::OutputPayload
/// [`ReplyPayload`]: crate::reply::ReplyPayload
/// [`SendOutput`]: crate::send::SendOutput
/// [`SendReply`]: crate::send::SendReply
/// [`Shutdown`]: crate::shutdown::Shutdown
pub mod prelude {
    pub use crate::descriptor::{
        DefinitionDescriptor, DefinitionKind, ItemKind, MessageDescriptor, OutputDescriptor,
        ReplyDescriptor,
    };
    pub use crate::provider::{ProviderRef, ProviderSpec};
    pub use crate::reply::{OutputPayload, ReplyPayload};
    pub use crate::send::{SendOutput, SendReply};
    pub use crate::shutdown::Shutdown;

    // Attribute macros — bring the `#[klipper_*]` annotations into scope
    // via `ankyra_macros` re-exports. The attribute forms of
    // `klipper_reply` and `klipper_output` are re-exported from
    // `ankyra_macros` directly; the `ankyra` crate reserves those names
    // for the fn-like call-site macros (`::ankyra::klipper_reply!(...)`).
    pub use crate::{klipper_command, klipper_constant};
    pub use ::ankyra_macros::{klipper_output, klipper_reply};
    // Function-like macros — bring `ankyra_provider!`, `ankyra_config!`,
    // and the `klipper_enumeration!` / `klipper_static_string!` /
    // `klipper_shutdown!` helpers into scope. `klipper_reply!` /
    // `klipper_output!` are deliberately omitted from the prelude because
    // their idents are reserved for the attribute re-exports above — users
    // invoke them via their fully qualified path
    // (`::ankyra::klipper_reply!(...)`). The `_from` variants have no
    // attribute-ident conflict, so they can safely live in the prelude.
    pub use crate::{
        ankyra_config, ankyra_provider, ankyra_reexport_provider, klipper_enumeration,
        klipper_output_from, klipper_reply_from, klipper_shutdown, klipper_shutdown_from,
        klipper_static_string,
    };
}
