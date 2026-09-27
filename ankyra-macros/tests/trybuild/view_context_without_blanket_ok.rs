//! Positive: a view-trait handler assembles when the context type
//! implements the view directly, with no blanket impl for `&mut T`.

use ankyra_macros::{ankyra_config, ankyra_provider, klipper_command};

pub trait ClockCtxView {
    fn now(&self) -> u32;
}

pub struct State;

impl ClockCtxView for State {
    fn now(&self) -> u32 {
        0
    }
}

impl ankyra::ShutdownState for State {
    fn is_shutdown(&self) -> bool {
        false
    }
}

#[klipper_command]
fn get_clock(ctx: &mut dyn ClockCtxView) {
    let _ = ctx.now();
}

ankyra_provider! {
    name: CORE_PROVIDER,
    commands: [get_clock],
}

pub struct BufferTransportOutput;
pub const TRANSPORT_OUTPUT: BufferTransportOutput = BufferTransportOutput;

impl ankyra::TransportOutput for BufferTransportOutput {
    type Output = ankyra::ScratchOutput<64>;
    fn output(&self, f: impl FnOnce(&mut Self::Output)) {
        let mut o = ankyra::ScratchOutput::<64>::new();
        f(&mut o);
    }
}

ankyra_config! {
    transport = crate::TRANSPORT_OUTPUT: crate::BufferTransportOutput,
    context = &'ctx mut crate::State,
    providers = [crate::CORE_PROVIDER],
    static_strings = [],
}

fn main() {}
