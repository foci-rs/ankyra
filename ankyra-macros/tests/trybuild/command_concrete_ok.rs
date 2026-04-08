use ankyra_macros::klipper_command;

pub struct State;

#[klipper_command]
fn emergency_stop(_ctx: &mut State) {}

fn main() {
    let _ = __ankyra_dispatch_emergency_stop::<()>;
}
