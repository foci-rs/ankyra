use ankyra::descriptor::DefinitionKind;
use ankyra_macros::klipper_constant;

#[klipper_constant]
pub const MCU: &str = "stm32f407";

fn main() {
    // The original const is preserved as a passthrough.
    assert_eq!(MCU, "stm32f407");

    // The descriptor fn exists at module scope and returns a DefinitionDescriptor.
    // `MCU` is a single-word all-uppercase ident and must be preserved
    // verbatim — Klipper's host reads `mcu` from the data dictionary's
    // `config` section exactly as the firmware emits it. See
    // `ankyra_codegen::pascal_to_snake`.
    let desc = __ankyra_descriptor_MCU();
    assert_eq!(desc.kind(), DefinitionKind::Constant);
    assert_eq!(desc.exported_name(), "MCU");
    assert_eq!(desc.value(), "stm32f407");
}
