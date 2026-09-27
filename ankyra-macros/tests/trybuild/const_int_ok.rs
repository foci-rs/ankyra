use ankyra::descriptor::DefinitionKind;
use ankyra_macros::klipper_constant;

#[klipper_constant]
pub const CLOCK_FREQ: u32 = 168_000_000;

fn main() {
    // The original const is preserved as a passthrough.
    assert_eq!(CLOCK_FREQ, 168_000_000);

    // The descriptor fn exists at module scope and returns a DefinitionDescriptor.
    // SCREAMING_SNAKE_CASE idents like `CLOCK_FREQ` are preserved
    // verbatim on the wire — Klipper's host looks them up via
    // `get_constant_float("CLOCK_FREQ")`, so lowercasing would break
    // the connect handshake. See `ankyra_codegen::pascal_to_snake`.
    let desc = __ankyra_descriptor_CLOCK_FREQ();
    assert_eq!(desc.kind(), DefinitionKind::Constant);
    assert_eq!(desc.exported_name(), "CLOCK_FREQ");
    assert_eq!(desc.value(), "168000000");
}
