//! Positive: #[klipper_output] inside a submodule compiles.

pub mod api {
    use ankyra_macros::klipper_output;
    #[klipper_output(format = "tick t=%u")]
    pub struct Tick {
        pub t: u32,
    }
}

fn main() {}
