//! Sender traits for replies and outputs.
//!
//! The assembler generates a concrete sender type that implements
//! [`SendReply<R>`] for every reply payload type `R` and [`SendOutput<O>`]
//! for every output payload type `O` declared across the firmware's
//! providers. Command handlers hold a `&mut` to this sender and dispatch
//! through these traits.

/// Send a reply payload to the host.
pub trait SendReply<R> {
    /// Consume `payload` and write it to the wire.
    fn send(&mut self, payload: R);
}

/// Send an unsolicited output payload to the host.
pub trait SendOutput<O> {
    /// Consume `payload` and write it to the wire.
    fn send(&mut self, payload: O);
}
