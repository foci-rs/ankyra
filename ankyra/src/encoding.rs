//! Wire encoding primitives for the Klipper protocol.
//!
//! The byte layout is ported verbatim from anchor's `encoding.rs` and must
//! match Klipper's on-wire variable-length quantity (VLQ) format exactly.
//!
//! The VLQ format is deliberately built on wrapping bit-level casts between
//! signed and unsigned integers, so the `clippy::cast_*` pedantic lints are
//! silenced at module scope rather than at every call site.

#![allow(
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

use crate::input_buffer::InputBuffer;
use crate::output_buffer::OutputBuffer;

/// Error type for representing a failed read.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ReadError;

/// Trait implemented for types that can be read from an [`InputBuffer`].
pub trait Readable: Sized {
    /// Attempt to read a `Self` from the input buffer, advancing the buffer
    /// past the consumed bytes on success.
    ///
    /// On failure, the amount by which the buffer has advanced is
    /// unspecified and callers should treat the remaining buffer contents
    /// as garbage.
    fn read(input: &mut impl InputBuffer) -> Result<Self, ReadError>;
}

/// Trait implemented for types that can be written to an [`OutputBuffer`].
pub trait Writable {
    /// Output the value to an [`OutputBuffer`].
    ///
    /// This operation cannot fail, and `OutputBuffer` has no way to indicate
    /// an overfull buffer. The buffer will simply be truncated and the
    /// resulting message will be invalid, likely causing the remote end to
    /// error out. It is the caller's responsibility to avoid this situation.
    fn write(&self, output: &mut impl OutputBuffer);
}

/// Pull the next byte from the input buffer, consuming it.
fn next_byte(input: &mut impl InputBuffer) -> Result<u8, ReadError> {
    let b = {
        let data = input.data();
        if data.is_empty() {
            return Err(ReadError);
        }
        data[0]
    };
    input.pop(1);
    Ok(b)
}

/// Parse a Klipper-style variable-length integer.
///
/// Layout (identical to anchor's `parse_vlq_int`):
/// - each byte contributes 7 bits of payload
/// - the MSB (`0x80`) signals continuation
/// - the first byte is sign-extended: if bits `0x60` are both set on the
///   first byte, the value is extended with the high 26 bits of `-1`
fn parse_vlq_int(input: &mut impl InputBuffer) -> Result<u32, ReadError> {
    let mut c = u32::from(next_byte(input)?);
    let mut v = c & 0x7F;
    if (c & 0x60) == 0x60 {
        v |= (-0x20_i32) as u32;
    }
    while c & 0x80 != 0 {
        c = u32::from(next_byte(input)?);
        v = (v << 7) | (c & 0x7F);
    }
    Ok(v)
}

/// Encode an integer using Klipper's VLQ layout.
///
/// Ported verbatim from anchor's `encode_vlq_int`. The continuation bytes
/// emitted depend on whether the signed representation falls outside the
/// per-length ranges.
fn encode_vlq_int(output: &mut impl OutputBuffer, v: u32) {
    let sv = v as i32;
    if !(-(1 << 26)..(3 << 26)).contains(&sv) {
        output.output(&[((sv >> 28) & 0x7F) as u8 | 0x80]);
    }
    if !(-(1 << 19)..(3 << 19)).contains(&sv) {
        output.output(&[((sv >> 21) & 0x7F) as u8 | 0x80]);
    }
    if !(-(1 << 12)..(3 << 12)).contains(&sv) {
        output.output(&[((sv >> 14) & 0x7F) as u8 | 0x80]);
    }
    if !(-(1 << 5)..(3 << 5)).contains(&sv) {
        output.output(&[((sv >> 7) & 0x7F) as u8 | 0x80]);
    }
    output.output(&[(sv & 0x7F) as u8]);
}

macro_rules! int_readwrite {
    ($type:ty) => {
        impl Readable for $type {
            fn read(input: &mut impl InputBuffer) -> Result<Self, ReadError> {
                parse_vlq_int(input).map(|v| v as $type)
            }
        }

        impl Writable for $type {
            fn write(&self, output: &mut impl OutputBuffer) {
                encode_vlq_int(output, *self as u32);
            }
        }
    };
}

int_readwrite!(u32);
int_readwrite!(i32);
int_readwrite!(u16);
int_readwrite!(i16);
int_readwrite!(u8);

impl Readable for bool {
    fn read(input: &mut impl InputBuffer) -> Result<Self, ReadError> {
        parse_vlq_int(input).map(|v| v != 0)
    }
}

impl Writable for bool {
    fn write(&self, output: &mut impl OutputBuffer) {
        encode_vlq_int(output, u32::from(*self));
    }
}

impl Writable for &[u8] {
    fn write(&self, output: &mut impl OutputBuffer) {
        encode_vlq_int(output, self.len() as u32);
        output.output(self);
    }
}

impl Writable for &str {
    fn write(&self, output: &mut impl OutputBuffer) {
        let bytes = self.as_bytes();
        encode_vlq_int(output, bytes.len() as u32);
        output.output(bytes);
    }
}
