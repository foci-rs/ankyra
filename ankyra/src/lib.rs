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
