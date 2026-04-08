use ankyra_macros::klipper_command;

pub trait ClockCtxView {
    fn now(&self) -> u32;
}

#[klipper_command]
fn get_clock(_ctx: &mut dyn ClockCtxView) {}

fn main() {
    // Force references to the generated items so they are type-checked.
    let _ = __ankyra_dispatch_get_clock::<()>;
}
