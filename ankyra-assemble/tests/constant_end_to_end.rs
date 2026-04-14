//! End-to-end integration test for crate-root `#[klipper_constant]` items.
//!
//! Exercises the full pipeline for constants: `#[klipper_constant]` emits
//! its sibling `pub const __ANKYRA_NAME_constant_<N>` /
//! `__ANKYRA_VALUE_constant_<N>` consts → `ankyra_provider!` registers
//! the constant at a crate-root path → `ankyra_config!` threads the
//! sibling scope (`$crate`) through the wrapped-carrier form →
//! `__ankyra_assemble!` stitches `<scope>::__ANKYRA_NAME/VALUE_*` into
//! `concatcp!` → the JSON dictionary surfaces the correct key and
//! value.
//!
//! The regression this test locks down is the "silently empty config
//! entry" bug that existed before the assembler started carrying
//! `sibling_scope` for all carrier-backed items. A provider listing a
//! renamed or missing `#[klipper_constant]` now produces a compile
//! error; a correctly-wired one round-trips the value through the
//! compressed dictionary bytes.

#![allow(clippy::cast_lossless)]

use ankyra::{ScratchOutput, TransportOutput, ankyra_config};
use ankyra_macros::{ankyra_provider, klipper_constant};

// --- User-defined constants at the crate root -------------------------------

#[klipper_constant]
pub const CLOCK_FREQ: u32 = 84_000_000;

#[klipper_constant]
pub const MCU: &str = "stm32f407xx";

ankyra_provider! {
    name: TEST_PROVIDER,
    constants: [CLOCK_FREQ, MCU],
}

// --- Minimal transport sink -------------------------------------------------
//
// The assembler's `ankyra_config!` requires a transport binding. Constants
// never traverse the transport so a zero-sized sink is sufficient; we only
// need the macros to expand cleanly so `_ankyra_config::DICT_BYTES` exists.

#[derive(Copy, Clone)]
pub struct NullOutput;

impl TransportOutput for NullOutput {
    type Output = ScratchOutput<16>;
    fn output(&self, f: impl FnOnce(&mut Self::Output)) {
        let mut scratch = ScratchOutput::<16>::new();
        f(&mut scratch);
    }
}

pub const TRANSPORT_OUTPUT: NullOutput = NullOutput;

ankyra_config! {
    transport = crate::TRANSPORT_OUTPUT: crate::NullOutput,
    context = &'ctx mut (),
    providers = [crate::TEST_PROVIDER],
    static_strings = [],
}

// --- Helpers ----------------------------------------------------------------

/// Decompress `DICT_BYTES` (zlib-deflate stream) into a JSON string and
/// return the `config` object as a string slice without parsing. This
/// keeps the test dependency-free beyond `flate2` (already used by
/// `end_to_end.rs`).
fn decompress_dict() -> String {
    use std::io::Read;
    let mut scratch =
        vec![0u8; ankyra::dictionary::max_compressed_size(_ankyra_config::DICT_BYTES.len())];
    let n = ankyra::dictionary::compress_dict_to(_ankyra_config::DICT_BYTES, &mut scratch)
        .expect("compression fits in max_compressed_size buffer");
    let mut decoder = flate2::read::ZlibDecoder::new(&scratch[..n]);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .expect("compressed dictionary is valid zlib");
    String::from_utf8(out).expect("dictionary is UTF-8")
}

/// Extract the contents of the `"config":{...}` object as a raw slice,
/// without a JSON parser. Returns everything between the opening and
/// matching closing brace.
fn extract_config_section(json: &str) -> &str {
    let marker = r#""config":{"#;
    let start = json.find(marker).expect("config section present") + marker.len();
    // Find the matching `}` that closes the config object. The config
    // values are bare numbers and quoted strings — no nested objects —
    // so a simple brace counter starting at 1 suffices.
    let tail = &json[start..];
    let mut depth: i32 = 1;
    let mut in_str = false;
    let mut esc = false;
    for (idx, ch) in tail.char_indices() {
        if esc {
            esc = false;
            continue;
        }
        match ch {
            '\\' if in_str => esc = true,
            '"' => in_str = !in_str,
            '{' if !in_str => depth += 1,
            '}' if !in_str => {
                depth -= 1;
                if depth == 0 {
                    return &tail[..idx];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated config section in dictionary: {tail}");
}

// --- Tests ------------------------------------------------------------------

#[test]
fn dict_bytes_compiled_without_errors() {
    // Smoke test: the macros produce a non-empty dictionary. If a
    // `compile_error!` had leaked through the sibling-scope path (e.g.
    // a provider listing a renamed const) this integration crate would
    // have failed to compile, so reaching this assertion already
    // exercises the hard-error wiring in the negative case.
    assert!(
        core::str::from_utf8(_ankyra_config::DICT_BYTES)
            .expect("DICT_BYTES must be valid UTF-8")
            .starts_with('{'),
        "DICT_BYTES must be emitted as JSON"
    );
}

#[test]
fn config_section_contains_integer_constant() {
    // Round-trip CLOCK_FREQ through the compressed dictionary and assert
    // the entry appears verbatim. The key must be the exact SCREAMING_SNAKE
    // ident (no pascal_to_snake lowercasing) and the value must be the
    // bare integer the `#[klipper_constant]` macro emitted.
    let json = decompress_dict();
    let config = extract_config_section(&json);
    assert!(
        config.contains(r#""CLOCK_FREQ":84000000"#),
        "CLOCK_FREQ integer entry missing from config section: {config}"
    );
}

#[test]
fn config_section_contains_string_constant() {
    // Round-trip the `&str`-typed constant: the value is JSON-quoted
    // by the `(value)` arm of the constant's carrier macro, so the
    // emitted pair is `"MCU":"stm32f407xx"`.
    let json = decompress_dict();
    let config = extract_config_section(&json);
    assert!(
        config.contains(r#""MCU":"stm32f407xx""#),
        "MCU string entry missing from config section: {config}"
    );
}

#[test]
fn config_section_contains_only_listed_constants() {
    // The provider registers exactly two constants. A regression that
    // silently added phantom entries (or dropped registered ones) would
    // surface here. We count commas at depth 0 inside the config block
    // to determine the entry count without a JSON parser.
    let json = decompress_dict();
    let config = extract_config_section(&json);
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut esc = false;
    let mut commas = 0usize;
    for ch in config.chars() {
        if esc {
            esc = false;
            continue;
        }
        match ch {
            '\\' if in_str => esc = true,
            '"' => in_str = !in_str,
            '{' if !in_str => depth += 1,
            '}' if !in_str => depth -= 1,
            ',' if !in_str && depth == 0 => commas += 1,
            _ => {}
        }
    }
    // Two entries → exactly one top-level comma separating them.
    assert_eq!(
        commas, 1,
        "expected exactly two entries (one comma) in config section, got {commas}: {config}"
    );
}
