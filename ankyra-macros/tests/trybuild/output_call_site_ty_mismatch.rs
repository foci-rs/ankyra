use ankyra_macros::{klipper_command, klipper_output};

#[klipper_output]
pub struct Tick {
    pub count: u32,
}

pub struct State;

#[klipper_command]
fn tick(_ctx: &mut State) {
    ::ankyra::klipper_output!(Tick, count: u8 = 7);
}

fn main() {}
