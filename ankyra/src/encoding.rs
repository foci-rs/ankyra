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

use crate::output_buffer::OutputBuffer;

/// Error type for representing a failed read.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ReadError;

/// Trait implemented for types that can be read from an input message.
///
/// The `'de` lifetime allows the implementation to return references to the original data buffer.
/// This permits zero-copy reading of variable length data like byte arrays and strings.
///
/// On failure, `data` may have been partially advanced; callers should treat the cursor as
/// unspecified and not retry with the same buffer.
pub trait Readable<'de>: Sized {
    fn read(data: &mut &'de [u8]) -> Result<Self, ReadError>;
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

/// Pull the next byte from the input cursor, consuming it.
fn next_byte(data: &mut &[u8]) -> Result<u8, ReadError> {
    if data.is_empty() {
        Err(ReadError)
    } else {
        let v = data[0];
        *data = &data[1..];
        Ok(v)
    }
}

/// Parse a Klipper-style variable-length integer.
///
/// Layout (identical to anchor's `parse_vlq_int`):
/// - each byte contributes 7 bits of payload
/// - the MSB (`0x80`) signals continuation
/// - the first byte is sign-extended: if bits `0x60` are both set on the
///   first byte, the value is extended with the high 26 bits of `-1`
fn parse_vlq_int(data: &mut &[u8]) -> Result<u32, ReadError> {
    let mut c = u32::from(next_byte(data)?);
    let mut v = c & 0x7F;
    if (c & 0x60) == 0x60 {
        v |= (-0x20_i32) as u32;
    }
    while c & 0x80 != 0 {
        c = u32::from(next_byte(data)?);
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
        impl Readable<'_> for $type {
            fn read(data: &mut &[u8]) -> Result<Self, ReadError> {
                parse_vlq_int(data).map(|v| v as $type)
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

impl Readable<'_> for bool {
    fn read(data: &mut &[u8]) -> Result<Self, ReadError> {
        parse_vlq_int(data).map(|v| v != 0)
    }
}

impl Writable for bool {
    fn write(&self, output: &mut impl OutputBuffer) {
        encode_vlq_int(output, u32::from(*self));
    }
}

impl<'de> Readable<'de> for &'de [u8] {
    fn read(data: &mut &'de [u8]) -> Result<Self, ReadError> {
        let len = parse_vlq_int(data)? as usize;
        if data.len() < len {
            Err(ReadError)
        } else {
            let ret = &data[..len];
            *data = &data[len..];
            Ok(ret)
        }
    }
}

impl<'de> Readable<'de> for &'de str {
    fn read(data: &mut &'de [u8]) -> Result<Self, ReadError> {
        let bytes = <&[u8] as Readable>::read(data)?;
        core::str::from_utf8(bytes).map_err(|_| ReadError)
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
