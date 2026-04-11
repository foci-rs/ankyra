// CRC16 and VLQ helpers below mirror the transport's byte-level arithmetic
// without depending on private crate internals, matching the pattern used
// by `end_to_end.rs`.
#![allow(
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::doc_markdown
)]

//! Integration test for the `in_shutdown` dispatch gate.
//!
//! Verifies three contracts simultaneously:
//!
//! 1. A command tagged `#[klipper_command(in_shutdown)]` is forwarded to
//!    its handler even when the context reports `is_shutdown() == true`.
//! 2. A command with the default attribute is dropped (handler not
//!    invoked) when `is_shutdown() == true`.
//! 3. The same default command is forwarded when `is_shutdown() == false`.
//!
//! The test is wired through a real `Transport<Config>` produced by
//! `ankyra_config!`, so it exercises the full macro-generated dispatch
//! table rather than inspecting the emitted token stream.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use ankyra::{ScratchOutput, ShutdownState, SliceInputBuffer, TransportOutput, ankyra_config};
use ankyra_macros::{ankyra_provider, klipper_command};

// --- Shared observability ---------------------------------------------------

/// Set to `true` on each `status_ping` invocation. Reset by the test.
static STATUS_PING_INVOKED: AtomicBool = AtomicBool::new(false);
/// Set to `true` on each `set_timer` invocation.
static SET_TIMER_INVOKED: AtomicBool = AtomicBool::new(false);
/// Captures the argument the handler saw. Proves the frame cursor was
/// advanced past the `u16` command id before forwarding to the handler.
static SET_TIMER_ARG: AtomicU32 = AtomicU32::new(0);

/// Serialises test execution. `KLIPPER_TRANSPORT` is a shared static that
/// tracks its own per-session sequence counter; running the gate tests
/// in parallel would cause later frames to be rejected when their seq
/// bytes do not match the counter the previous test left behind. Tests
/// acquire this guard before driving `receive` and reset the counter
/// indirectly by holding a single sequence window per test.
static TEST_LOCK: Mutex<()> = Mutex::new(());

fn acquire_test_lock() -> MutexGuard<'static, ()> {
    // Some tests intentionally panic on assertion failure; recover a
    // poisoned lock so a single failure does not cascade into
    // "secondary" failures that obscure the real cause.
    TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Next sequence byte the shared `KLIPPER_TRANSPORT` expects. Incremented
/// after every successful frame dispatch; tests hold `TEST_LOCK` while
/// reading + bumping this counter so the wire stays in-sync across
/// serialised test runs.
static NEXT_SEQ: AtomicU32 = AtomicU32::new(0);

fn next_seq() -> u8 {
    (NEXT_SEQ.fetch_add(1, Ordering::SeqCst) & 0x0F) as u8
}

// --- User-defined context + protocol items ---------------------------------

/// Minimal context type implementing `ShutdownState` with a flag the test
/// flips before driving `Transport::receive`.
pub struct TestCtx {
    pub in_shutdown: bool,
}

impl ShutdownState for TestCtx {
    fn is_shutdown(&self) -> bool {
        self.in_shutdown
    }
}

/// Default-attribute command. Expected to be dropped by the generated
/// dispatch when `ctx.is_shutdown() == true`.
#[klipper_command]
fn set_timer(_ctx: &mut TestCtx, ticks: u32) {
    SET_TIMER_INVOKED.store(true, Ordering::SeqCst);
    SET_TIMER_ARG.store(ticks, Ordering::SeqCst);
}

/// `in_shutdown`-marked command. Expected to run regardless of the
/// context's shutdown state — this is the "status/recovery" class of
/// commands the FOCI firmware relies on (e.g. `get_clock`,
/// `emergency_stop`, `clear_shutdown`).
#[klipper_command(in_shutdown)]
fn status_ping(_ctx: &mut TestCtx) {
    STATUS_PING_INVOKED.store(true, Ordering::SeqCst);
}

ankyra_provider! {
    name: SHUTDOWN_GATE_PROVIDER,
    commands: [set_timer, status_ping],
}

// --- Transport output sink --------------------------------------------------

static CAPTURE_BUF: Mutex<[u8; 512]> = Mutex::new([0u8; 512]);
static CAPTURE_LEN: AtomicUsize = AtomicUsize::new(0);

#[derive(Copy, Clone)]
pub struct CapturingOutput;

impl TransportOutput for CapturingOutput {
    type Output = ScratchOutput<128>;
    fn output(&self, f: impl FnOnce(&mut Self::Output)) {
        let mut scratch = ScratchOutput::<128>::new();
        f(&mut scratch);
        let result = scratch.result();
        let mut guard = CAPTURE_BUF.lock().unwrap();
        let prev = CAPTURE_LEN.load(Ordering::SeqCst);
        let copy_len = result.len().min(guard.len() - prev);
        guard[prev..prev + copy_len].copy_from_slice(&result[..copy_len]);
        CAPTURE_LEN.store(prev + copy_len, Ordering::SeqCst);
    }
}

pub const TRANSPORT_OUTPUT: CapturingOutput = CapturingOutput;

// --- Assembler invocation ----------------------------------------------------

ankyra_config! {
    transport = crate::TRANSPORT_OUTPUT: crate::CapturingOutput,
    context = &'ctx mut crate::TestCtx,
    providers = [crate::SHUTDOWN_GATE_PROVIDER],
    static_strings = [],
}

// --- Helpers ---------------------------------------------------------------

fn crc16(buf: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for b in buf {
        let b = *b ^ ((crc & 0xFF) as u8);
        let b = b ^ (b << 4);
        let b16 = b as u16;
        crc = (b16 << 8 | crc >> 8) ^ (b16 >> 4) ^ (b16 << 3);
    }
    crc
}

fn encode_frame(payload: &[u8], seq: u8) -> Vec<u8> {
    const MESSAGE_DEST: u8 = 0x10;
    const MESSAGE_VALUE_SYNC: u8 = 0x7E;
    let len = 2 + payload.len() + 3;
    let seq_byte = (seq & 0x0F) | MESSAGE_DEST;
    let mut out = Vec::with_capacity(len);
    out.push(len as u8);
    out.push(seq_byte);
    out.extend_from_slice(payload);
    let crc = crc16(&out);
    out.push(((crc & 0xFF00) >> 8) as u8);
    out.push((crc & 0xFF) as u8);
    out.push(MESSAGE_VALUE_SYNC);
    out
}

fn encode_vlq_u32(v: u32, out: &mut Vec<u8>) {
    let sv = v as i32;
    if !(-(1 << 26)..(3 << 26)).contains(&sv) {
        out.push(((sv >> 28) & 0x7F) as u8 | 0x80);
    }
    if !(-(1 << 19)..(3 << 19)).contains(&sv) {
        out.push(((sv >> 21) & 0x7F) as u8 | 0x80);
    }
    if !(-(1 << 12)..(3 << 12)).contains(&sv) {
        out.push(((sv >> 14) & 0x7F) as u8 | 0x80);
    }
    if !(-(1 << 5)..(3 << 5)).contains(&sv) {
        out.push(((sv >> 7) & 0x7F) as u8 | 0x80);
    }
    out.push((sv & 0x7F) as u8);
}

/// Reset the observability flags before each sub-scenario. The statics
/// persist across tests, so sharing a single per-test cleanup point is
/// safer than relying on declaration order.
fn reset_flags() {
    STATUS_PING_INVOKED.store(false, Ordering::SeqCst);
    SET_TIMER_INVOKED.store(false, Ordering::SeqCst);
    SET_TIMER_ARG.store(0, Ordering::SeqCst);
}

/// Dictionary id of a command by its message-format key. `ankyra_config!`
/// assigns ids by canonical sort order, which is kind-major then ASCII
/// name-minor — looking up through the public `DICT_BYTES` gives us a
/// stable, assembler-agnostic way to derive the id at test time rather
/// than hard-coding the expected number.
fn lookup_command_id(format_key: &str) -> u16 {
    use ankyra::dictionary::compress_dict_to;
    use std::io::Read;
    // Decompress the dictionary the same way the host would. This is
    // overkill for id lookup, but keeps the test robust against future
    // changes to the compression shape.
    let mut scratch =
        vec![0u8; ankyra::dictionary::max_compressed_size(_ankyra_config::DICT_BYTES.len())];
    let n = compress_dict_to(_ankyra_config::DICT_BYTES, &mut scratch)
        .expect("dictionary compresses within max_compressed_size");
    let mut decoder = flate2::read::ZlibDecoder::new(&scratch[..n]);
    let mut round = Vec::new();
    decoder.read_to_end(&mut round).expect("valid zlib stream");
    let json = core::str::from_utf8(&round).expect("dictionary is UTF-8");
    // Find `"<format_key>":<id>` in the `commands` section.
    let needle = format!(r#""{format_key}":"#);
    let idx = json
        .find(&needle)
        .unwrap_or_else(|| panic!("command `{format_key}` missing from dictionary: {json}"));
    let rest = &json[idx + needle.len()..];
    // Read digits until the next non-digit terminator.
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end]
        .parse::<u16>()
        .expect("command id parses as u16")
}

fn set_timer_payload(ticks: u32) -> Vec<u8> {
    let id = lookup_command_id("set_timer ticks=%u");
    let mut out = Vec::new();
    encode_vlq_u32(id as u32, &mut out);
    encode_vlq_u32(ticks, &mut out);
    out
}

fn status_ping_payload() -> Vec<u8> {
    let id = lookup_command_id("status_ping");
    let mut out = Vec::new();
    encode_vlq_u32(id as u32, &mut out);
    out
}

// --- Tests -----------------------------------------------------------------

#[test]
fn default_command_runs_when_context_not_in_shutdown() {
    let _guard = acquire_test_lock();
    reset_flags();
    let framed = encode_frame(&set_timer_payload(42), next_seq());
    let mut input = SliceInputBuffer::new(&framed);
    let mut ctx = TestCtx { in_shutdown: false };
    KLIPPER_TRANSPORT.receive(&mut input, &mut ctx);
    assert!(
        SET_TIMER_INVOKED.load(Ordering::SeqCst),
        "set_timer must run when context is not in shutdown"
    );
    assert_eq!(
        SET_TIMER_ARG.load(Ordering::SeqCst),
        42,
        "handler must observe the argument decoded from the frame"
    );
}

#[test]
fn default_command_dropped_when_context_in_shutdown() {
    let _guard = acquire_test_lock();
    reset_flags();
    let framed = encode_frame(&set_timer_payload(99), next_seq());
    let mut input = SliceInputBuffer::new(&framed);
    let mut ctx = TestCtx { in_shutdown: true };
    KLIPPER_TRANSPORT.receive(&mut input, &mut ctx);
    assert!(
        !SET_TIMER_INVOKED.load(Ordering::SeqCst),
        "default command must be dropped when context reports is_shutdown()"
    );
    assert_eq!(
        SET_TIMER_ARG.load(Ordering::SeqCst),
        0,
        "handler must not observe any argument when gated off"
    );
}

#[test]
fn in_shutdown_command_runs_regardless_of_shutdown_state() {
    let _guard = acquire_test_lock();
    // Shutdown = true: the in_shutdown command must still run.
    reset_flags();
    let framed = encode_frame(&status_ping_payload(), next_seq());
    let mut input = SliceInputBuffer::new(&framed);
    let mut ctx = TestCtx { in_shutdown: true };
    KLIPPER_TRANSPORT.receive(&mut input, &mut ctx);
    assert!(
        STATUS_PING_INVOKED.load(Ordering::SeqCst),
        "status_ping must run when ctx.is_shutdown() is true"
    );

    // And again with shutdown = false to confirm the default branch.
    reset_flags();
    let framed = encode_frame(&status_ping_payload(), next_seq());
    let mut input = SliceInputBuffer::new(&framed);
    let mut ctx = TestCtx { in_shutdown: false };
    KLIPPER_TRANSPORT.receive(&mut input, &mut ctx);
    assert!(
        STATUS_PING_INVOKED.load(Ordering::SeqCst),
        "status_ping must also run when ctx.is_shutdown() is false"
    );
}

#[test]
fn in_shutdown_sibling_const_reflects_attribute() {
    // Sanity check on the compile-time surface: the two sibling bool
    // constants the dispatch emitter consults must carry the right
    // values. Clippy's `assertions_on_constants` lint flags these
    // because the values are known at compile time — which is exactly
    // the point. Moving the checks into a `const {}` block converts
    // them into compile-time errors if either const ever drifts, which
    // is the strongest guarantee available.
    const _: () = assert!(__ANKYRA_IN_SHUTDOWN_status_ping);
    const _: () = assert!(!__ANKYRA_IN_SHUTDOWN_set_timer);
}
