use ankyra_macros::klipper_command;

pub trait ClockCtxView {
    fn now(&self) -> u32;
}

pub struct State;
// Intentionally missing: impl ClockCtxView for State

#[klipper_command]
fn get_clock(_ctx: &mut dyn ClockCtxView) {}

fn main() {
    let mut s = State;
    let mut frame: &[u8] = &[];
    let mut sender = ();
    // Calling the dispatch wrapper coerces `&mut State` to
    // `&mut dyn ClockCtxView`, which fails with E0277 because
    // `State: !ClockCtxView`.
    let _ = __ankyra_dispatch_get_clock::<()>(&mut frame, &mut s, &mut sender);
}
