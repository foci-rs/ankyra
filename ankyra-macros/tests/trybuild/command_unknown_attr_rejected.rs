// `#[klipper_command(...)]` only accepts the bare ident `in_shutdown` or
// an empty argument list. Anything else must be rejected with a
// span-accurate compile error so typos don't silently widen the
// shutdown-gated surface.
use ankyra_macros::klipper_command;

pub struct State;

#[klipper_command(unknown_option)]
fn bogus(_ctx: &mut State) {}

fn main() {}
