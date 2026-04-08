use ankyra_macros::klipper_command;

pub trait View {}

#[klipper_command]
fn peek(_ctx: &dyn View) {}

fn main() {}
