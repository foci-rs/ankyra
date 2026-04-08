use ankyra_macros::klipper_output;

// `%u` expects `u32`, but the field is `u16` (which would need `%hu`).
#[klipper_output(format = "blip v=%u")]
pub struct Blip {
    pub v: u16,
}

fn main() {}
