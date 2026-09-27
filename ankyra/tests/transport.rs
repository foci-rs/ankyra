//! Integration test covering the end-to-end receive flow.
//!
//! Builds a synthetic Klipper frame using a local copy of the `crc16` helper
//! and feeds it to `Transport::receive`. A test `Config` captures the
//! dispatched command id so the test can assert the state machine decoded the
//! frame correctly.

#![allow(clippy::cast_lossless, clippy::cast_possible_truncation)]

use ankyra::encoding::{ReadError, Writable};
use ankyra::transport::{Config, ShutdownState, Transport};
use ankyra::transport_output::TransportOutput;
use ankyra::{ScratchOutput, SliceInputBuffer};
use core::cell::{Cell, RefCell};

/// Captures bytes emitted by the transport's ACK path for debugging.
struct CapturingOutput {
    buf: RefCell<[u8; 256]>,
    len: Cell<usize>,
}

impl CapturingOutput {
    fn new() -> Self {
        Self {
            buf: RefCell::new([0u8; 256]),
            len: Cell::new(0),
        }
    }
}

impl TransportOutput for CapturingOutput {
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

/// Records the last dispatched command id so the test can assert on it.
struct TestContext<'a> {
    dispatched_cmd: &'a Cell<Option<u16>>,
}

impl ShutdownState for TestContext<'_> {
    fn is_shutdown(&self) -> bool {
        false
    }
}

struct TestConfig;

impl Config for TestConfig {
    type TransportOutput = CapturingOutput;
    type Context<'c> = TestContext<'c>;
    fn dispatch(
        cmd: u16,
        _frame: &mut &[u8],
        context: &mut Self::Context<'_>,
    ) -> Result<(), ReadError> {
        context.dispatched_cmd.set(Some(cmd));
        Ok(())
    }
}

/// Local copy of the transport's private `crc16`, so the test can build a
/// valid frame without depending on transport internals.
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

const HEADER_SIZE: usize = 2;
const TRAILER_SIZE: usize = 3;
const DEST_SEQ_0: u8 = 0x10;
const SYNC: u8 = 0x7E;

/// Build a full Klipper frame wrapping the given VLQ payload.
fn build_frame(payload: &[u8]) -> ([u8; 64], usize) {
    let mut out = [0u8; 64];
    let body_end = HEADER_SIZE + payload.len();
    let total_len = body_end + TRAILER_SIZE;
    assert!(total_len <= 64, "test helper only builds small frames");
    out[0] = total_len as u8;
    out[1] = DEST_SEQ_0;
    out[HEADER_SIZE..body_end].copy_from_slice(payload);
    let crc = crc16(&out[..body_end]);
    out[body_end] = ((crc & 0xFF00) >> 8) as u8;
    out[body_end + 1] = (crc & 0xFF) as u8;
    out[body_end + 2] = SYNC;
    (out, total_len)
}

#[test]
fn dispatch_invoked_with_command_id() {
    let mut scratch = ScratchOutput::<8>::new();
    <u16 as Writable>::write(&42u16, &mut scratch);
    let payload = scratch.result();
    assert_eq!(payload, &[0x2A]);

    let (frame, total_len) = build_frame(payload);

    let transport = Transport::<TestConfig>::new(&TestConfig, CapturingOutput::new());
    let dispatched = Cell::new(None);
    let ctx = TestContext {
        dispatched_cmd: &dispatched,
    };

    let mut input = SliceInputBuffer::new(&frame[..total_len]);
    transport.receive(&mut input, ctx);

    assert_eq!(
        dispatched.get(),
        Some(42),
        "dispatch should be called with cmd id 42",
    );
}
