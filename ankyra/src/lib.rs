#![cfg_attr(not(feature = "std"), no_std)]

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

/// Convenience re-exports for end users.
pub mod prelude {
    pub use crate::descriptor::{
        DefinitionDescriptor, DefinitionKind, ItemKind, MessageDescriptor, OutputDescriptor,
        ReplyDescriptor,
    };
    pub use crate::provider::{ProviderRef, ProviderSpec};
    pub use crate::reply::{OutputPayload, ReplyPayload};
    pub use crate::send::{SendOutput, SendReply};
    pub use crate::shutdown::Shutdown;
}
