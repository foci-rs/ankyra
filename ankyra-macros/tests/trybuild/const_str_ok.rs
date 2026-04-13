use ankyra::descriptor::DefinitionKind;
use ankyra_macros::klipper_constant;

#[klipper_constant]
pub const MCU: &str = "stm32f407";

fn main() {
    // The original const is preserved as a passthrough.
    assert_eq!(MCU, "stm32f407");

    // The descriptor fn exists at module scope and returns a DefinitionDescriptor.
    // `MCU` is auto-lowercased to `mcu` on the wire; see
    // `shared::pascal_to_snake`.
    let desc = __ankyra_descriptor_MCU();
    assert_eq!(desc.kind(), DefinitionKind::Constant);
    assert_eq!(desc.exported_name(), "mcu");
    assert_eq!(desc.value(), "stm32f407");
}
