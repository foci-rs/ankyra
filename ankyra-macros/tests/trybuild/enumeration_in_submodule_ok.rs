//! Positive: klipper_enumeration! inside a submodule compiles.

pub mod api {
    use ankyra_macros::klipper_enumeration;
    klipper_enumeration! {
        pub enum MotorKind {
            Stepper,
            Bldc,
        }
    }
}

fn main() {}
