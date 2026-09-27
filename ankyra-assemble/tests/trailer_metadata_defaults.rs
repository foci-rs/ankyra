use ankyra::{ScratchOutput, TransportOutput, ankyra_config};
use ankyra_macros::{ankyra_provider, klipper_command};

#[klipper_command]
fn ping_default(_ctx: &mut ()) {}

ankyra_provider! {
    name: DEFAULT_PROVIDER,
    commands: [ping_default],
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

ankyra_config! {
    transport = crate::TRANSPORT_OUTPUT: crate::NullOutput,
    context = &'ctx mut (),
    providers = [crate::DEFAULT_PROVIDER],
}

#[test]
fn trailer_uses_ankyra_defaults_when_no_overrides_given() {
    let json = core::str::from_utf8(_ankyra_config::DICT_BYTES).expect("dictionary is valid UTF-8");

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
    let last_part = json
        .rfind(r#""version":"ankyra-v0.1""#)
        .expect("version field present");
    let tail = &json[last_part..];
    assert!(
        tail.contains(r#""app":"ankyra""#) && tail.contains(r#""license":"MIT OR Apache-2.0""#),
        "version/app/license must appear in canonical order: {tail}"
    );
}
