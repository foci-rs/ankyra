//! Marker traits for reply and output payloads.
//!
//! These are implemented by payload types produced via `#[klipper_reply]`
//! and `#[klipper_output]` (and by the built-in [`Shutdown`] reply). They
//! exist so the assembler can bound generated sender code on "this type is
//! a reply payload" without pulling in the full `Writable` signature.
//!
//! [`Shutdown`]: crate::shutdown::Shutdown

/// Marker trait for types that can be sent as a reply.
pub trait ReplyPayload {}

/// Marker trait for types that can be sent as an unsolicited output.
pub trait OutputPayload {}
