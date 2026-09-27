//! Negative: Klipper keys output messages by format string, so two
//! `#[klipper_output]` structs with the same format must not assemble.

use ankyra_macros::{ankyra_config, ankyra_provider, klipper_command, klipper_output};

#[klipper_command]
fn ping(_ctx: &mut ()) {}

#[klipper_output(format = "tick n=%u")]
pub struct Tick {
    pub n: u32,
}

#[klipper_output(format = "tick n=%u")]
pub struct Tock {
    pub n: u32,
}

ankyra_provider! {
    name: COLLIDING_PROVIDER,
    commands: [ping],
    outputs: [Tick, Tock],
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
    context = &'ctx mut (),
    providers = [crate::COLLIDING_PROVIDER],
    static_strings = [],
}

fn main() {}
