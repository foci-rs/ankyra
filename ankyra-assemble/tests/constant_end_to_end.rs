use ankyra::{ScratchOutput, TransportOutput, ankyra_config};
use ankyra_macros::{ankyra_provider, klipper_constant};

#[klipper_constant]
pub const CLOCK_FREQ: u32 = 84_000_000;

#[klipper_constant]
pub const MCU: &str = "stm32f407xx";

ankyra_provider! {
    name: TEST_PROVIDER,
    constants: [CLOCK_FREQ, MCU],
}

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

fn decompress_dict() -> String {
    use std::io::Read;
    let mut decoder = flate2::read::ZlibDecoder::new(_ankyra_config::COMPRESSED_DICT.as_slice());
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .expect("compressed dictionary is valid zlib");
    String::from_utf8(out).expect("dictionary is UTF-8")
}

fn extract_config_section(json: &str) -> &str {
    let marker = r#""config":{"#;
    let start = json.find(marker).expect("config section present") + marker.len();
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

#[test]
fn dict_bytes_compiled_without_errors() {
    assert!(
        core::str::from_utf8(_ankyra_config::DICT_BYTES)
            .expect("DICT_BYTES must be valid UTF-8")
            .starts_with('{'),
        "DICT_BYTES must be emitted as JSON"
    );
}

#[test]
fn config_section_contains_integer_constant() {
    let json = decompress_dict();
    let config = extract_config_section(&json);
    assert!(
        config.contains(r#""CLOCK_FREQ":84000000"#),
        "CLOCK_FREQ integer entry missing from config section: {config}"
    );
}

#[test]
fn config_section_contains_string_constant() {
    let json = decompress_dict();
    let config = extract_config_section(&json);
    assert!(
        config.contains(r#""MCU":"stm32f407xx""#),
        "MCU string entry missing from config section: {config}"
    );
}

#[test]
fn config_section_contains_only_listed_constants() {
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
    assert_eq!(
        commas, 1,
        "expected exactly two entries (one comma) in config section, got {commas}: {config}"
    );
}
