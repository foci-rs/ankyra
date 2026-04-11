//! Positive: #[klipper_constant] inside a submodule compiles.

pub mod api {
    use ankyra_macros::klipper_constant;
    #[klipper_constant]
    pub const FREQ: u32 = 1_000_000;
}

fn main() {}
