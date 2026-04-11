//! Positive: #[klipper_command] inside a submodule compiles.

use ankyra_macros::klipper_command;

pub mod api {
    use super::*;
    #[klipper_command]
    pub fn get_clock(_ctx: &mut ()) {}
}

fn main() {}
