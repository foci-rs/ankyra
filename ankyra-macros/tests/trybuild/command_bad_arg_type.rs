use ankyra_macros::klipper_command;

pub struct State;

#[klipper_command]
fn bad(_ctx: &mut State, bad_arg: f32) {
    let _ = bad_arg;
}

fn main() {}
