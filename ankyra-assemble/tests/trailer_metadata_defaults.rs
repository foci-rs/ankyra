//! Integration test: `ankyra_config!` without any of the four trailer
//! metadata overrides (`app`, `version`, `build_versions`, `license`)
//! must emit the exact ankyra defaults in the data dictionary JSON,
//! matching the pre-configurable-metadata wire format byte-for-byte.
//!
//! This test lives in its own file (rather than as another assertion in
//! `end_to_end.rs`) because `ankyra_config!` expands to a `_ankyra_config`
//! module + a `KLIPPER_TRANSPORT` re-export. Each test crate can only
//! host one expansion, so each shape variant needs its own `tests/*.rs`
//! file.

use ankyra::{ScratchOutput, TransportOutput, ankyra_config};
use ankyra_macros::{ankyra_provider, klipper_command};

#[klipper_command]
fn ping_default(_ctx: &mut ()) {}

ankyra_provider! {
    name: DEFAULT_PROVIDER,
    commands: [ping_default],
}

/// Zero-sized sink. The transport output is not exercised here — this
/// test reads `DICT_BYTES` directly to assert the trailer shape.
#[derive(Copy, Clone)]
pub struct NullOutput;

impl TransportOutput for NullOutput {
    type Output = ScratchOutput<32>;
    fn output(&self, f: impl FnOnce(&mut Self::Output)) {
        let mut o = ScratchOutput::<32>::new();
        f(&mut o);
    }
}

pub const TRANSPORT_OUTPUT: NullOutput = NullOutput;

// Deliberately no `app`, `version`, `build_versions`, or `license` keys —
// every default must kick in.
ankyra_config! {
    transport = crate::TRANSPORT_OUTPUT: crate::NullOutput,
    context = &'ctx mut (),
    providers = [crate::DEFAULT_PROVIDER],
}

#[test]
fn trailer_uses_ankyra_defaults_when_no_overrides_given() {
    let json = core::str::from_utf8(_ankyra_config::DICT_BYTES).expect("dictionary is valid UTF-8");

    // `build_versions` default resolves via the `ankyra-assemble` crate's
    // own `CARGO_PKG_VERSION` at its compile site, not the consuming
    // crate's. Assert the prefix so the test does not need to track
    // ankyra-assemble's version number.
    assert!(
        json.contains(r#""version":"ankyra-v0.1""#),
        "default version must be `ankyra-v0.1`: {json}"
    );
    assert!(
        json.contains(r#""build_versions":"ankyra-"#),
        "default build_versions must start with `ankyra-<version>`: {json}"
    );
    assert!(
        json.contains(r#""app":"ankyra""#),
        "default app must be `ankyra`: {json}"
    );
    assert!(
        json.contains(r#""license":"MIT OR Apache-2.0""#),
        "default license must be `MIT OR Apache-2.0`: {json}"
    );
    // The full trailer must appear in the canonical field order.
    let last_part = json
        .rfind(r#""version":"ankyra-v0.1""#)
        .expect("version field present");
    let tail = &json[last_part..];
    assert!(
        tail.contains(r#""app":"ankyra""#) && tail.contains(r#""license":"MIT OR Apache-2.0""#),
        "version/app/license must appear in canonical order: {tail}"
    );
}
