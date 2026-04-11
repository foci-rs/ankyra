// Verify `#[klipper_command(in_shutdown)]` parses and emits a dispatch
// wrapper plus a sibling `__ANKYRA_IN_SHUTDOWN_<name>: bool` constant.
//
// The bool const is consumed by the assembler's dispatch emitter at
// rustc-typecheck time to elide the shutdown gate for status/recovery
// commands like `get_clock`, `emergency_stop`, and `clear_shutdown`.
use ankyra_macros::klipper_command;

pub struct State;

#[klipper_command(in_shutdown)]
fn get_clock(_ctx: &mut State) {}

#[klipper_command]
fn set_timer(_ctx: &mut State, _ticks: u32) {}

fn main() {
    // Force references so the generated items are type-checked.
    let _ = __ankyra_dispatch_get_clock::<()>;
    let _ = __ankyra_dispatch_set_timer::<()>;
    // The sibling bool constants are the single source of truth the
    // dispatch emitter consults when deciding whether to gate.
    let _: bool = __ANKYRA_IN_SHUTDOWN_get_clock;
    let _: bool = __ANKYRA_IN_SHUTDOWN_set_timer;
    assert!(__ANKYRA_IN_SHUTDOWN_get_clock);
    assert!(!__ANKYRA_IN_SHUTDOWN_set_timer);
}
