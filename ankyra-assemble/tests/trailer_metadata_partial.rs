//! Integration test: `ankyra_config!` with only some of the trailer
//! metadata overrides set. The keys the user supplied must appear with
//! the user's values; the omitted keys must fall through to ankyra's
//! defaults — independently per-key.

use ankyra::{ScratchOutput, TransportOutput, ankyra_config};
use ankyra_macros::{ankyra_provider, klipper_command};

#[klipper_command]
fn ping_partial(_ctx: &mut ()) {}

ankyra_provider! {
    name: PARTIAL_PROVIDER,
    commands: [ping_partial],
}

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

// Only `app` is overridden; `version`, `build_versions`, and `license`
// must take ankyra's defaults.
ankyra_config! {
    transport = crate::TRANSPORT_OUTPUT: crate::NullOutput,
    context = &'ctx mut (),
    providers = [crate::PARTIAL_PROVIDER],
    app = "foci",
}

#[test]
fn partial_override_only_replaces_the_supplied_key() {
    let json = core::str::from_utf8(_ankyra_config::DICT_BYTES).expect("dictionary is valid UTF-8");

    // `app` was overridden — it must carry the user's value.
    assert!(
        json.contains(r#""app":"foci""#),
        "app override must be applied: {json}"
    );
    assert!(
        !json.contains(r#""app":"ankyra""#),
        "app default must not leak through when override is set: {json}"
    );

    // The other three trailer fields were omitted — ankyra defaults apply.
    assert!(
        json.contains(r#""version":"ankyra-v0.1""#),
        "version must fall through to default: {json}"
    );
    assert!(
        json.contains(r#""build_versions":"ankyra-"#),
        "build_versions must fall through to default: {json}"
    );
    assert!(
        json.contains(r#""license":"MIT OR Apache-2.0""#),
        "license must fall through to default: {json}"
    );
}
