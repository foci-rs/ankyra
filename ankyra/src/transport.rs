//! Transport framing, CRC, and dispatch contract.
//!
//! Ported verbatim from anchor's `transport.rs`. The constants, CRC16, and
//! receive state machine all match Klipper's MCU protocol byte-for-byte.
//!
//! The key surface here is the [`Config`] trait, which downstream assembler
//! macros implement to plug command dispatch into the transport, and
//! [`Transport`] itself, which owns the receive-side synchronization state
//! and the output sink.
//!
//! The integer casts inside `crc16`, `encode_acknak`, and `encode_frame` are
//! deliberate bit-level truncations that mirror anchor's implementation, so
//! the relevant pedantic lints are silenced at module scope to avoid cluttering
//! the code with individual `allow` attributes.

#![allow(
    // Anchor's CRC and frame arithmetic rely on deliberate bit-level casts
    // that do not round-trip through wider types.
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    // Literal port of anchor's state machine: the `if !is_synchronized`
    // branch is ordered to match Klipper exactly; swapping the arms to
    // appease `if_not_else` would diverge from the reference implementation.
    clippy::if_not_else,
    // `encode_frame` forwards the output's return value for parity with
    // anchor; a trailing unit expression reads more clearly than `;`.
    clippy::semicolon_if_nothing_returned,
    // `parse_frame` is a method for API symmetry with `encode_frame` and
    // `encode_acknak`, even though the port does not touch `self`.
    clippy::unused_self,
    // `debug_assert!` format string ported verbatim from anchor.
    clippy::uninlined_format_args,
    // `encode_frame` identifier inside rustdoc refers to the in-scope method.
    clippy::doc_markdown,
)]

use crate::encoding::{ReadError, Readable};
use crate::input_buffer::InputBuffer;
use crate::output_buffer::OutputBuffer;
use crate::transport_output::TransportOutput;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

const MESSAGE_HEADER_SIZE: usize = 2;
const MESSAGE_TRAILER_SIZE: usize = 3;
const MESSAGE_LENGTH_MIN: usize = MESSAGE_HEADER_SIZE + MESSAGE_TRAILER_SIZE;
const MESSAGE_LENGTH_MAX: usize = 64;
const MESSAGE_POSITION_LENGTH: usize = 0;
const MESSAGE_POSITION_SEQ: usize = 1;
const MESSAGE_TRAILER_CRC: usize = 3;
const MESSAGE_TRAILER_SYNC: usize = 1;
const MESSAGE_VALUE_SYNC: u8 = 0x7E;
const MESSAGE_DEST: u8 = 0x10;
const MESSAGE_SEQ_MASK: u8 = 0x0F;

/// CRC16 used on every Klipper frame. Ported byte-for-byte from anchor.
fn crc16(buf: &[u8]) -> u16 {
    let mut crc = 0xFFFFu16;
    for b in buf {
        let b = *b ^ ((crc & 0xFF) as u8);
        let b = b ^ (b << 4);
        let b16 = b as u16;
        crc = ((b16 << 8) | (crc >> 8)) ^ (b16 >> 4) ^ (b16 << 3);
    }
    crc
}

/// Trait for context types that can report MCU shutdown state.
///
/// When the MCU is in shutdown, commands not marked with `in_shutdown`
/// are silently dropped by the generated dispatcher.
pub trait ShutdownState {
    fn is_shutdown(&self) -> bool;
}

impl ShutdownState for () {
    fn is_shutdown(&self) -> bool {
        false
    }
}

impl<T: ShutdownState> ShutdownState for &mut T {
    fn is_shutdown(&self) -> bool {
        (**self).is_shutdown()
    }
}

#[cfg(test)]
mod shutdown_tests {
    use super::ShutdownState;

    struct MockContext {
        shutdown: bool,
    }

    impl ShutdownState for MockContext {
        fn is_shutdown(&self) -> bool {
            self.shutdown
        }
    }

    #[test]
    fn unit_type_never_shutdown() {
        assert!(!().is_shutdown());
    }

    #[test]
    fn mock_not_shutdown() {
        let ctx = MockContext { shutdown: false };
        assert!(!ctx.is_shutdown());
    }

    #[test]
    fn mock_shutdown() {
        let ctx = MockContext { shutdown: true };
        assert!(ctx.is_shutdown());
    }

    #[test]
    fn mut_ref_delegates_to_inner() {
        let mut ctx = MockContext { shutdown: true };
        let r: &mut MockContext = &mut ctx;
        assert!(r.is_shutdown());
    }
}

/// Glue trait implemented by the assembler to wire dispatch and output
/// sinks into a [`Transport`].
pub trait Config {
    /// The sink that outbound frames (ACK/NAK and data) are written to.
    type TransportOutput: TransportOutput;
    /// The per-call context threaded through dispatch. Must be able to report
    /// MCU shutdown state so the generated dispatcher can drop non-shutdown
    /// commands when appropriate.
    type Context<'c>: ShutdownState;
    /// Route a decoded command id and its remaining argument bytes to the
    /// appropriate handler. Called by [`Transport::receive`].
    fn dispatch(
        cmd: u16,
        frame: &mut &[u8],
        context: &mut Self::Context<'_>,
    ) -> Result<(), ReadError>;
}

/// Protocol transport implementation.
pub struct Transport<C: Config + 'static> {
    is_synchronized: AtomicBool,
    next_sequence: AtomicU8,
    output: C::TransportOutput,
}

impl<C: Config> Transport<C> {
    #[doc(hidden)]
    pub const fn new(_config: &'static C, output: C::TransportOutput) -> Self {
        Self {
            is_synchronized: AtomicBool::new(true),
            next_sequence: AtomicU8::new(MESSAGE_DEST),
            output,
        }
    }

    /// Decode messages from an [`InputBuffer`].
    pub fn receive(&self, input: &mut impl InputBuffer, mut context: C::Context<'_>) {
        // Drive state machine forward until we either have no
        // input or know we don't have enough input.
        let mut data = input.data();
        while !data.is_empty() {
            if !self.is_synchronized.load(Ordering::SeqCst) {
                // Look for a sync byte
                if let Some(n) = data.iter().position(|b| *b == MESSAGE_VALUE_SYNC) {
                    data = &data[n + 1..];
                    self.is_synchronized.store(true, Ordering::SeqCst);
                    self.encode_acknak();
                } else {
                    data = &[];
                }
            } else {
                if data[0] == MESSAGE_VALUE_SYNC {
                    data = &data[1..];
                    continue;
                }

                if data.len() < MESSAGE_LENGTH_MIN {
                    break;
                }

                let len = data[MESSAGE_POSITION_LENGTH] as usize;
                if !(MESSAGE_LENGTH_MIN..=MESSAGE_LENGTH_MAX).contains(&len) {
                    self.is_synchronized.store(false, Ordering::SeqCst);
                    continue;
                }

                let seq = data[MESSAGE_POSITION_SEQ];
                if seq & !MESSAGE_SEQ_MASK != MESSAGE_DEST {
                    self.is_synchronized.store(false, Ordering::SeqCst);
                    continue;
                }
                if data.len() < len {
                    break;
                }
                if data[len - MESSAGE_TRAILER_SYNC] != MESSAGE_VALUE_SYNC {
                    self.is_synchronized.store(false, Ordering::SeqCst);
                    continue;
                }

                let frame_crc = ((data[len - MESSAGE_TRAILER_CRC] as u16) << 8)
                    | (data[len - MESSAGE_TRAILER_CRC + 1] as u16);
                let actual_crc = crc16(&data[0..len - MESSAGE_TRAILER_SIZE]);
                if frame_crc != actual_crc {
                    self.is_synchronized.store(false, Ordering::SeqCst);
                    continue;
                }

                let frame = &data[MESSAGE_HEADER_SIZE..len - MESSAGE_TRAILER_SIZE];
                data = &data[len..];
                if seq == self.next_sequence.load(Ordering::SeqCst) {
                    self.next_sequence.store(
                        ((seq + 1) & MESSAGE_SEQ_MASK) | MESSAGE_DEST,
                        Ordering::SeqCst,
                    );
                    let _ = self.parse_frame(frame, &mut context);
                }
                self.encode_acknak();
            }
        }
        // Remove consumed bytes from front
        let consumed = input.available() - data.len();
        if consumed > 0 {
            input.pop(consumed);
        }
    }

    fn parse_frame(&self, mut frame: &[u8], context: &mut C::Context<'_>) -> Result<(), ReadError> {
        while !frame.is_empty() {
            let cmd = <u16 as Readable>::read(&mut frame)?;
            C::dispatch(cmd, &mut frame, context)?;
        }
        Ok(())
    }

    // Fast path for ACK/NAK
    fn encode_acknak(&self) {
        self.output.output(|output| {
            let ns = self.next_sequence.load(Ordering::SeqCst);
            let crc = crc16(&[5, ns]);
            output.output(&[
                5,
                ns,
                ((crc & 0xFF00) >> 8) as u8,
                (crc & 0xFF) as u8,
                MESSAGE_VALUE_SYNC,
            ]);
        });
    }

    #[doc(hidden)]
    pub fn encode_frame(
        &self,
        f: impl FnOnce(&mut <<C as Config>::TransportOutput as TransportOutput>::Output),
    ) {
        self.output.output(|output| {
            let cursor = output.cur_position();
            output.output(&[0, self.next_sequence.load(Ordering::SeqCst)]); // Output header
            f(output); // Output actual frame contents
            {
                let changed = output.data_since(cursor).len();
                let frame_len = changed + MESSAGE_TRAILER_SIZE;
                if frame_len > MESSAGE_LENGTH_MAX {
                    // Oversized frame — roll back all bytes written since
                    // cursor so no partial data leaks into the TX stream.
                    output.rollback(cursor);
                    debug_assert!(
                        false,
                        "frame length {} exceeds protocol max {}",
                        frame_len, MESSAGE_LENGTH_MAX
                    );
                    return;
                }
                output.update(cursor, frame_len as u8);
            }
            let crc = crc16(output.data_since(cursor));
            output.output(&[
                ((crc & 0xFF00) >> 8) as u8,
                (crc & 0xFF) as u8,
                MESSAGE_VALUE_SYNC,
            ]);
        })
    }
}

#[cfg(test)]
mod encode_frame_tests {
    use super::*;
    use crate::output_buffer::ScratchOutput;
    use core::cell::{Cell, RefCell};

    /// Captures bytes emitted by encode_frame for test assertions.
    struct TestOutput {
        buf: RefCell<[u8; 256]>,
        len: Cell<usize>,
    }

    impl TestOutput {
        fn new() -> Self {
            Self {
                buf: RefCell::new([0u8; 256]),
                len: Cell::new(0),
            }
        }
        fn output_len(&self) -> usize {
            self.len.get()
        }
        fn first_byte(&self) -> u8 {
            self.buf.borrow()[0]
        }
        fn last_byte(&self) -> u8 {
            let l = self.len.get();
            if l > 0 { self.buf.borrow()[l - 1] } else { 0 }
        }
    }

    impl TransportOutput for TestOutput {
        type Output = ScratchOutput<256>;
        fn output(&self, f: impl FnOnce(&mut Self::Output)) {
            let mut scratch = ScratchOutput::new();
            f(&mut scratch);
            let result = scratch.result();
            let prev = self.len.get();
            let copy_len = result.len().min(256 - prev);
            self.buf.borrow_mut()[prev..prev + copy_len].copy_from_slice(&result[..copy_len]);
            self.len.set(prev + copy_len);
        }
    }

    struct TestConfig;

    impl Config for TestConfig {
        type TransportOutput = TestOutput;
        type Context<'c> = ();
        fn dispatch(
            _cmd: u16,
            _frame: &mut &[u8],
            _context: &mut Self::Context<'_>,
        ) -> Result<(), ReadError> {
            Ok(())
        }
    }

    #[test]
    fn normal_frame_emits_bytes() {
        let output = TestOutput::new();
        let transport = Transport::<TestConfig>::new(&TestConfig, output);
        transport.encode_frame(|buf| {
            buf.output(&[0x01, 0x02]); // small payload
        });
        assert!(transport.output.output_len() > 0);
        assert_eq!(transport.output.last_byte(), MESSAGE_VALUE_SYNC);
    }

    /// Verifies that oversized frames produce zero output bytes after
    /// rollback. Uses catch_unwind to absorb the debug_assert panic
    /// so we can inspect output state afterward.
    #[cfg(feature = "std")]
    #[test]
    fn oversized_frame_emits_zero_bytes() {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        let output = TestOutput::new();
        let transport = Transport::<TestConfig>::new(&TestConfig, output);
        let result = catch_unwind(AssertUnwindSafe(|| {
            transport.encode_frame(|buf| {
                // 62 bytes payload + 2 header + 3 trailer = 67 > 64
                buf.output(&[0xAA; 62]);
            });
        }));
        // debug_assert fires in debug builds
        assert!(
            result.is_err(),
            "expected debug_assert panic for oversized frame"
        );
        // Rollback ensures zero bytes reached the output
        assert_eq!(
            transport.output.output_len(),
            0,
            "oversized frame should produce zero output bytes after rollback"
        );
    }

    #[test]
    fn max_valid_frame_emits_bytes() {
        let output = TestOutput::new();
        let transport = Transport::<TestConfig>::new(&TestConfig, output);
        transport.encode_frame(|buf| {
            // 59 bytes payload + 2 header + 3 trailer = 64 = MESSAGE_LENGTH_MAX
            buf.output(&[0xBB; 59]);
        });
        assert!(transport.output.output_len() > 0);
        assert_eq!(transport.output.first_byte(), 64); // length = MAX
        assert_eq!(transport.output.last_byte(), MESSAGE_VALUE_SYNC);
    }
}
