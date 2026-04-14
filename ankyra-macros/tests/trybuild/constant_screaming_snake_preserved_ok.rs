//! Positive: SCREAMING_SNAKE_CASE `#[klipper_constant]` idents pass
//! through `pascal_to_snake` verbatim.
//!
//! Klipper's host looks constants up by name (for example
//! `get_constant_float("CLOCK_FREQ")`), so any lowercasing of the ident
//! breaks the connect handshake with `Firmware constant 'CLOCK_FREQ'
//! not found`. FOCI's baseline data dictionary shows every
//! `#[klipper_constant]` in SCREAMING_SNAKE_CASE on the wire.
//!
//! This fixture asserts via the descriptor fn emitted by
//! `#[klipper_constant]` that the exported wire name matches the
//! source ident exactly — for multi-word idents, single-word
//! all-uppercase idents, and idents with embedded digits.

use ankyra::descriptor::DefinitionKind;
use ankyra_macros::klipper_constant;

#[klipper_constant]
pub const CLOCK_FREQ: u32 = 84_000_000;

#[klipper_constant]
pub const RESERVE_PINS_USB: &str = "PA11,PA12";

#[klipper_constant]
pub const STATS_SUMSQ_BASE: u32 = 256;

#[klipper_constant]
pub const MCU: &str = "stm32f407";

#[klipper_constant]
pub const DATA_32BIT: u32 = 32;

fn main() {
    let desc = __ankyra_descriptor_CLOCK_FREQ();
    assert_eq!(desc.kind(), DefinitionKind::Constant);
    assert_eq!(desc.exported_name(), "CLOCK_FREQ");
    assert_eq!(desc.value(), "84000000");

    let desc = __ankyra_descriptor_RESERVE_PINS_USB();
    assert_eq!(desc.exported_name(), "RESERVE_PINS_USB");
    assert_eq!(desc.value(), "PA11,PA12");

    let desc = __ankyra_descriptor_STATS_SUMSQ_BASE();
    assert_eq!(desc.exported_name(), "STATS_SUMSQ_BASE");
    assert_eq!(desc.value(), "256");

    let desc = __ankyra_descriptor_MCU();
    assert_eq!(desc.exported_name(), "MCU");
    assert_eq!(desc.value(), "stm32f407");

    let desc = __ankyra_descriptor_DATA_32BIT();
    assert_eq!(desc.exported_name(), "DATA_32BIT");
    assert_eq!(desc.value(), "32");
}
