//! The built-in `shutdown` reply.
//!
//! Every firmware must be able to emit a `shutdown` reply to the host when
//! an unrecoverable error occurs. The wire format is fixed by Klipper:
//! `shutdown clock=%u static_string_id=%hu`.

use crate::ReplyWireSize;
use crate::encoding::Writable;
use crate::output_buffer::OutputBuffer;
use crate::reply::ReplyPayload;

/// Built-in `shutdown` reply payload.
///
/// `clock` is the MCU clock at the time of the fault, and
/// `static_string_id` is the pre-registered reason string's ID.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Shutdown {
    /// MCU clock tick at the time of the fault.
    pub clock: u32,
    /// ID of a pre-registered static string describing the reason.
    pub static_string_id: u16,
}

impl ReplyPayload for Shutdown {}

impl ReplyWireSize for Shutdown {
    const MAX_PAYLOAD_BYTES: Option<usize> = Some(5 + 3);
}

impl Writable for Shutdown {
    fn write(&self, output: &mut impl OutputBuffer) {
        <u32 as Writable>::write(&self.clock, output);
        <u16 as Writable>::write(&self.static_string_id, output);
    }
}
