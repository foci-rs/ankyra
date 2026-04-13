//! Positive: a PascalCase `#[klipper_output]` struct ident is auto-converted
//! to snake_case on the wire when the user does not supply an explicit
//! `format = "..."`. The auto-converter also handles acronym runs
//! (`ADCValue` → `adc_value`).

use ankyra::descriptor::OutputDescriptor;
use ankyra_macros::klipper_output;

#[klipper_output]
pub struct ADCValue {
    pub raw: u32,
}

fn main() {
    let desc: OutputDescriptor = __ankyra_descriptor_ADCValue();
    assert_eq!(desc.protocol_name(), "adc_value");
    assert_eq!(desc.message_format(), "adc_value raw=%u");
}
