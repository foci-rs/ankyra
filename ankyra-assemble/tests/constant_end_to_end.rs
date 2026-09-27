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

fn config_section() -> serde_json::Map<String, serde_json::Value> {
    use std::io::Read;
    let mut decoder = flate2::read::ZlibDecoder::new(_ankyra_config::COMPRESSED_DICT.as_slice());
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .expect("compressed dictionary is valid zlib");
    let mut json: serde_json::Value =
        serde_json::from_slice(&out).expect("dictionary is valid JSON");
    match json["config"].take() {
        serde_json::Value::Object(config) => config,
        other => panic!("config section must be a JSON object, got {other}"),
    }
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
    let config = config_section();
    assert_eq!(
        config.get("CLOCK_FREQ").and_then(serde_json::Value::as_u64),
        Some(84_000_000),
        "CLOCK_FREQ integer entry missing from config section: {config:?}"
    );
}

#[test]
fn config_section_contains_string_constant() {
    let config = config_section();
    assert_eq!(
        config.get("MCU").and_then(serde_json::Value::as_str),
        Some("stm32f407xx"),
        "MCU string entry missing from config section: {config:?}"
    );
}

#[test]
fn config_section_contains_only_listed_constants() {
    let config = config_section();
    let mut keys: Vec<&str> = config.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["CLOCK_FREQ", "MCU"],
        "config section must hold exactly the listed constants: {config:?}"
    );
    let raw = core::str::from_utf8(_ankyra_config::DICT_BYTES).expect("DICT_BYTES is UTF-8");
    for key in keys {
        assert_eq!(
            raw.matches(&format!("\"{key}\":")).count(),
            1,
            "`{key}` must appear once; a parsed object hides duplicate keys"
        );
    }
}
