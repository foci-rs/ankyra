#![allow(
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use ankyra::{ScratchOutput, ShutdownState, SliceInputBuffer, TransportOutput, ankyra_config};
use ankyra_macros::{ankyra_provider, klipper_command};

static STATUS_PING_INVOKED: AtomicBool = AtomicBool::new(false);
static SET_TIMER_INVOKED: AtomicBool = AtomicBool::new(false);
static SET_TIMER_ARG: AtomicU32 = AtomicU32::new(0);

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn acquire_test_lock() -> MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

static NEXT_SEQ: AtomicU32 = AtomicU32::new(0);

fn next_seq() -> u8 {
    (NEXT_SEQ.fetch_add(1, Ordering::SeqCst) & 0x0F) as u8
}

pub struct TestCtx {
    pub in_shutdown: bool,
}

impl ShutdownState for TestCtx {
    fn is_shutdown(&self) -> bool {
        self.in_shutdown
    }
}

#[klipper_command]
fn set_timer(_ctx: &mut TestCtx, ticks: u32) {
    SET_TIMER_INVOKED.store(true, Ordering::SeqCst);
    SET_TIMER_ARG.store(ticks, Ordering::SeqCst);
}

#[klipper_command(in_shutdown)]
fn status_ping(_ctx: &mut TestCtx) {
    STATUS_PING_INVOKED.store(true, Ordering::SeqCst);
}

ankyra_provider! {
    name: SHUTDOWN_GATE_PROVIDER,
    commands: [set_timer, status_ping],
}

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

ankyra_config! {
    transport = crate::TRANSPORT_OUTPUT: crate::CapturingOutput,
    context = &'ctx mut crate::TestCtx,
    providers = [crate::SHUTDOWN_GATE_PROVIDER],
    static_strings = [],
}

fn crc16(buf: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for b in buf {
        let b = *b ^ ((crc & 0xFF) as u8);
        let b = b ^ (b << 4);
        let b16 = b as u16;
        crc = ((b16 << 8) | (crc >> 8)) ^ (b16 >> 4) ^ (b16 << 3);
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

fn reset_flags() {
    STATUS_PING_INVOKED.store(false, Ordering::SeqCst);
    SET_TIMER_INVOKED.store(false, Ordering::SeqCst);
    SET_TIMER_ARG.store(0, Ordering::SeqCst);
}

fn lookup_command_id(format_key: &str) -> u16 {
    use std::io::Read;
    let mut decoder = flate2::read::ZlibDecoder::new(_ankyra_config::COMPRESSED_DICT.as_slice());
    let mut round = Vec::new();
    decoder.read_to_end(&mut round).expect("valid zlib stream");
    let json: serde_json::Value = serde_json::from_slice(&round).expect("dictionary is valid JSON");
    let id = json["commands"][format_key]
        .as_u64()
        .unwrap_or_else(|| panic!("command `{format_key}` missing from dictionary: {json}"));
    u16::try_from(id).expect("command id fits in u16")
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
    reset_flags();
    let framed = encode_frame(&status_ping_payload(), next_seq());
    let mut input = SliceInputBuffer::new(&framed);
    let mut ctx = TestCtx { in_shutdown: true };
    KLIPPER_TRANSPORT.receive(&mut input, &mut ctx);
    assert!(
        STATUS_PING_INVOKED.load(Ordering::SeqCst),
        "status_ping must run when ctx.is_shutdown() is true"
    );

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
    const _: [(); 1] = [(); __ANKYRA_IN_SHUTDOWN_status_ping as usize];
    const _: [(); 1] = [(); (!__ANKYRA_IN_SHUTDOWN_set_timer) as usize];
}
