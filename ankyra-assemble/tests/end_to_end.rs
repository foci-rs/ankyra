// The hand-rolled CRC16 and VLQ helpers below are deliberate verbatim
// ports of the transport's byte-level arithmetic so the test does not
// depend on private crate internals. The same casts are already
// `allow`-listed on the ported module upstream.
#![allow(
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::doc_markdown,
    // The `set_pin(_ctx: &mut (), _oid: u8, _value: u8)` handler below
    // deliberately binds its args with leading underscores to mirror
    // real Klipper/FOCI handlers that silence the `unused_variables`
    // lint this way. The proc-macro-emitted dispatch wrapper reads
    // these bindings to verify the underscore stripping happens only
    // in the wire-format string, not in the Rust binding.
    clippy::used_underscore_binding
)]

//! End-to-end integration test for Task 12's terminal assembler.
//!
//! Exercises the full pipeline: `ankyra_config!` → CPS fold → `__ankyra_assemble!`
//! → real `Transport<Config>` plus dispatch, senders, dictionary, and
//! static strings. Then we encode a synthetic `identify(offset=0, count=64)`
//! frame, feed it to `KLIPPER_TRANSPORT.receive(...)`, and assert the
//! output starts with the expected `identify_response` reply prefix
//! (reply id 0 followed by offset 0 followed by dictionary bytes).
//!
//! # Why this test lives in `ankyra-assemble`
//!
//! Proc-macro crates cannot host integration tests that consume their own
//! output directly — cargo's test harness imports a proc-macro crate as a
//! dependency of the integration-test crate, which is this one. Writing
//! the test under `ankyra-assemble/tests/` gives us the necessary layer.
//! Because `__ankyra_assemble!` is only exported via the `ankyra` runtime
//! re-export, we invoke it through the same `ankyra::ankyra_config!`
//! users would reach for.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use ankyra::{ScratchOutput, SliceInputBuffer, TransportOutput, ankyra_config};
use ankyra_macros::{ankyra_provider, klipper_command, klipper_reply};

/// Shared capture buffer. `Transport::new` takes its output by value in a
/// `const fn`, so the top-level `TRANSPORT_OUTPUT` is a zero-sized forwarder
/// that writes into this global. The buffer itself needs `Sync` because it
/// backs a `pub static KLIPPER_TRANSPORT`.
static CAPTURE_BUF: Mutex<[u8; 512]> = Mutex::new([0u8; 512]);
static CAPTURE_LEN: AtomicUsize = AtomicUsize::new(0);

// --- User-defined protocol items ---------------------------------------------

#[klipper_command]
fn emergency_stop(_ctx: &mut ()) {}

// Handler whose args are prefixed with a leading underscore to silence
// the `unused_variables` lint. The wire format must strip the
// underscore (`_oid` → `oid`, `_value` → `value`) so Klipper's host
// does not reject the command with `Command format mismatch`.
#[klipper_command]
fn set_pin(_ctx: &mut (), _oid: u8, _value: u8) {
    let _ = (_oid, _value);
}

#[klipper_reply]
pub struct PingReply {
    pub seq: u32,
}

ankyra_provider! {
    name: CORE_PROVIDER,
    commands: [emergency_stop, set_pin],
    replies: [PingReply],
}

// --- Transport output sink capturing emitted bytes for inspection ------------

/// Zero-sized forwarder so the `const` can be inlined at every use site.
/// All state lives in the `CAPTURE_BUF` / `CAPTURE_LEN` statics.
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
    context = &'ctx mut (),
    providers = [crate::CORE_PROVIDER],
    static_strings = ["boom"],
    app = "clock-firmware",
    version = "custom-v1.2.3",
    build_versions = concat!("custom-", env!("CARGO_PKG_VERSION")),
    license = "Apache-2.0",
}

// --- Helpers ----------------------------------------------------------------

/// Klipper CRC16 (ported from the transport impl — duplicated so the test
/// does not depend on a private function).
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

/// Encode a Klipper frame wrapping `payload`. `seq` is the low 4 bits of
/// the sequence byte; `MESSAGE_DEST` is set on the high nibble.
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

/// VLQ-encode a single `u32` (mirror of `ankyra::encoding::encode_vlq_int`).
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

/// Build the payload for an `identify` command: cmd_id=1 followed by two
/// VLQ-encoded u32 arguments (offset, count).
fn identify_payload(offset: u32, count: u32) -> Vec<u8> {
    let mut out = Vec::new();
    encode_vlq_u32(1, &mut out); // identify cmd id
    encode_vlq_u32(offset, &mut out);
    encode_vlq_u32(count, &mut out);
    out
}

/// VLQ-decode a single `u32` from `data`, advancing the slice.
fn decode_vlq_u32(data: &mut &[u8]) -> u32 {
    let mut c = u32::from(data[0]);
    *data = &data[1..];
    let mut v = c & 0x7F;
    if (c & 0x60) == 0x60 {
        v |= (-0x20_i32) as u32;
    }
    while c & 0x80 != 0 {
        c = u32::from(data[0]);
        *data = &data[1..];
        v = (v << 7) | (c & 0x7F);
    }
    v
}

// --- Tests ------------------------------------------------------------------

#[test]
fn transport_is_real_value() {
    // Confirms Task 12 promoted `KLIPPER_TRANSPORT` from the Task 10 unit
    // placeholder to a real `Transport<Config>` (different type each time
    // but always generic over the firmware-local `Config`).
    let _: &ankyra::transport::Transport<_> = &KLIPPER_TRANSPORT;
}

#[test]
fn static_string_ids_assigned() {
    let id: u16 = ankyra::klipper_static_string!("boom");
    assert!(id >= 2, "user-listed static strings get ids >= 2; got {id}");
}

#[test]
fn dictionary_exports_static_string_id_enumeration() {
    let dict: &[u8] = _ankyra_config::DICT_BYTES;
    let json: serde_json::Value = serde_json::from_slice(dict).expect("dictionary is valid JSON");
    let expected_id = u64::from(ankyra::klipper_static_string!("boom"));

    let enumeration_id = json["enumerations"]["static_string_id"]["boom"]
        .as_u64()
        .expect("static_string_id enumeration entry must be an integer");
    assert_eq!(
        enumeration_id, expected_id,
        "static_string_id enumeration must mirror the assigned static-string id"
    );

    let string_table = json["static_strings"]["2"]
        .as_str()
        .expect("static_strings section must still map id back to content");
    assert_eq!(string_table, "boom");
}

#[test]
fn identify_response_contains_dictionary_bytes() {
    // Build a framed `identify(offset=0, count=40)` command and stream it
    // through the transport. The dispatcher should route cmd id 1 to
    // `handle_identify`, which zlib-compresses the dictionary and emits
    // an `identify_response` reply carrying the first `count` bytes of
    // the compressed stream. A single Klipper frame caps at 64 bytes
    // total — after the 2-byte header, 3-byte trailer, and reply-id +
    // offset + VLQ-length header overhead, ~40 bytes fit comfortably
    // inside one response.
    let payload = identify_payload(0, 40);
    let framed = encode_frame(&payload, 0);
    let mut input = SliceInputBuffer::new(&framed);

    // The capture buffer is static and shared across tests — for
    // determinism we snapshot the length before invoking receive and
    // slice at that position after.
    let before = CAPTURE_LEN.load(Ordering::SeqCst);
    KLIPPER_TRANSPORT.receive(&mut input, &mut ());
    let after = CAPTURE_LEN.load(Ordering::SeqCst);
    assert!(
        after > before,
        "receive must emit at least an ACK plus an identify_response"
    );

    let guard = CAPTURE_BUF.lock().unwrap();
    let emitted: Vec<u8> = guard[before..after].to_vec();
    drop(guard);
    let emitted = emitted.as_slice();
    // `Transport::receive` invokes the dispatcher (which sends the
    // identify_response frame) BEFORE it emits the trailing ACK, so the
    // capture buffer lays out [identify_response][ack]. The first byte is
    // therefore the length of the identify_response frame.
    assert!(
        emitted.len() >= 5,
        "identify_response frame missing; got {emitted:?}"
    );
    let len = emitted[0] as usize;
    assert!(
        emitted.len() >= len,
        "identify_response frame truncated: expected >= {len} bytes, got {}",
        emitted.len()
    );

    // Payload layout after header: reply_id (u16 VLQ), offset (u32 VLQ),
    // data_len (u32 VLQ), data...
    let payload = &emitted[2..len - 3];
    let mut cursor: &[u8] = payload;
    let reply_id = decode_vlq_u32(&mut cursor) as u16;
    assert_eq!(reply_id, 0, "identify_response must carry reply id 0");
    let offset = decode_vlq_u32(&mut cursor);
    assert_eq!(offset, 0, "first slice of dictionary must be at offset 0");
    let data_len = decode_vlq_u32(&mut cursor) as usize;
    assert!(
        data_len > 0,
        "identify_response must carry non-empty dictionary bytes"
    );
    let data = &cursor[..data_len];
    // The dictionary bytes are the head of a zlib stream (RFC 1950) —
    // Klipper's host pipes them through `zlib.decompress()` before
    // parsing JSON. The first byte is the `0x78` CMF/CINFO magic.
    assert_eq!(
        data[0], 0x78,
        "dictionary payload must start with zlib magic 0x78; got 0x{:02x}",
        data[0]
    );
    let header = u16::from_be_bytes([data[0], data[1]]);
    assert_eq!(header % 31, 0, "zlib FCHECK invalid");
}

#[test]
fn dictionary_contains_user_item_format_strings() {
    // D1 goal: the `#[klipper_reply] struct PingReply { seq: u32 }` in
    // this test crate contributes a Klipper-style format `"PingReply seq=%u"`
    // to the data dictionary's `responses` section. Verify by slurping
    // the whole dictionary (chained identify_response frames are an
    // integration concern; we can read the static directly for this
    // assertion).
    let dict: &[u8] = _ankyra_config::DICT_BYTES;
    let json = core::str::from_utf8(dict).expect("dictionary is valid UTF-8");

    // Authoritative command format (`emergency_stop` has no args so its
    // format is just the command name).
    assert!(
        json.contains(r#""emergency_stop":"#),
        "user command missing from commands section: {json}"
    );
    // Command with underscore-prefixed params: the wire format must
    // strip the leading underscore from each parameter name so
    // Klipper's host accepts it (`_oid` → `oid`, `_value` → `value`).
    assert!(
        json.contains(r#""set_pin oid=%c value=%c":"#),
        "underscore-prefixed params must be stripped in wire format: {json}"
    );
    assert!(
        !json.contains("_oid=%c"),
        "underscore must not leak into wire format: {json}"
    );
    // Reply format for `PingReply seq: u32` — the Klipper-style string
    // derived from the field types at macro-expansion time. The ident
    // `PingReply` auto-converts to the `ping_reply` wire name (see
    // `ankyra_macros::shared::pascal_to_snake`).
    assert!(
        json.contains(r#""ping_reply seq=%u":"#),
        "ping_reply format missing from responses section: {json}"
    );
    // The synthesized shutdown reply carries its Klipper-accurate format.
    assert!(
        json.contains(r#""shutdown clock=%u static_string_id=%hu":"#),
        "shutdown format missing: {json}"
    );
    // Top-level metadata reflects the custom overrides passed to
    // `ankyra_config!` above; defaults are exercised in the separate
    // `trailer_metadata_defaults.rs` and `trailer_metadata_partial.rs`
    // integration tests.
    assert!(
        json.contains(r#""app":"clock-firmware""#),
        "custom app override missing: {json}"
    );
    assert!(
        json.contains(r#""version":"custom-v1.2.3""#),
        "custom version override missing: {json}"
    );
    assert!(
        json.contains(r#""license":"Apache-2.0""#),
        "custom license override missing: {json}"
    );
    // `build_versions` was set to `concat!("custom-", env!("CARGO_PKG_VERSION"))`,
    // which resolves to `"custom-<ankyra-assemble version>"` (the `env!` is
    // evaluated at this test crate's compile site — which is
    // `ankyra-assemble` itself for an integration test under
    // `ankyra-assemble/tests/`).
    assert!(
        json.contains(r#""build_versions":"custom-"#),
        "custom build_versions override missing: {json}"
    );
}

#[test]
fn dictionary_trailer_matches_all_overrides() {
    // Positive case: every metadata key is supplied. The JSON trailer
    // must emit the four fields in ankyra's canonical order and carry
    // only the user-provided values.
    let json = core::str::from_utf8(_ankyra_config::DICT_BYTES).expect("dictionary is valid UTF-8");
    let expected_build_versions =
        format!(r#""build_versions":"custom-{}""#, env!("CARGO_PKG_VERSION"));
    let trailer_fragment = format!(
        r#""version":"custom-v1.2.3",{expected_build_versions},"app":"clock-firmware","license":"Apache-2.0""#,
    );
    assert!(
        json.contains(&trailer_fragment),
        "trailer ordering/content mismatch:\nexpected substring: {trailer_fragment}\n\
         dictionary: {json}"
    );
}

#[test]
fn identify_response_stream_decompresses_to_full_dictionary() {
    // D3 goal: assemble the full compressed dictionary the way the
    // host does, feed it through `flate2::read::ZlibDecoder`, and
    // confirm it round-trips to `DICT_BYTES` byte-for-byte. The
    // ankyra transport tracks its own sequence counter and therefore
    // is not straightforward to drive through many chained frames
    // across a shared static from a parallel test harness — so
    // instead of reissuing `identify` repeatedly through the real
    // transport, we invoke the same `compress_dict_to` helper that
    // `handle_identify` uses and decompress that. End-to-end framing
    // is covered by `identify_response_contains_dictionary_bytes`.
    use std::io::Read;

    let mut scratch =
        vec![0u8; ankyra::dictionary::max_compressed_size(_ankyra_config::DICT_BYTES.len())];
    let n = ankyra::dictionary::compress_dict_to(_ankyra_config::DICT_BYTES, &mut scratch)
        .expect("compression fits in max_compressed_size buffer");
    let mut decoder = flate2::read::ZlibDecoder::new(&scratch[..n]);
    let mut round = Vec::new();
    decoder
        .read_to_end(&mut round)
        .expect("compressed dictionary is valid zlib");
    assert_eq!(
        round.as_slice(),
        _ankyra_config::DICT_BYTES,
        "decompressed stream must match DICT_BYTES"
    );
    // Sanity: the decompressed JSON starts with an object brace.
    assert_eq!(round[0], b'{');
}
