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
pub use transport::{ShutdownState, Transport};
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
    // (`::ankyra::klipper_reply!(...)`).
    pub use crate::{
        ankyra_config, ankyra_provider, ankyra_reexport_provider, klipper_enumeration,
        klipper_shutdown, klipper_static_string,
    };
}
