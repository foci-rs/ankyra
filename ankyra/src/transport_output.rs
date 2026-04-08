//! `TransportOutput` trait: the sink for outbound protocol bytes.
//!
//! Ported verbatim from anchor's `transport_output.rs`. Transports (and
//! higher-level assembler output helpers) call `output` with a closure that
//! fills a caller-supplied [`OutputBuffer`]. The implementation decides what
//! to do with the fully-formed bytes once the closure returns.

use crate::output_buffer::OutputBuffer;

/// Trait representing the capability to serialize an output message.
pub trait TransportOutput {
    /// The type of [`OutputBuffer`] that will be provided to the caller.
    type Output: OutputBuffer;

    /// Request output of a message.
    ///
    /// The `f` callback will be called with an empty [`OutputBuffer`] that
    /// must be filled with the message to be sent.
    fn output(&self, f: impl FnOnce(&mut Self::Output));
}

impl<T> TransportOutput for &T
where
    T: TransportOutput,
{
    type Output = T::Output;
    fn output(&self, f: impl FnOnce(&mut Self::Output)) {
        (*self).output(f);
    }
}
