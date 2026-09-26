// Diagnostic fixture: aggregating a command and a reply that share
// the same protocol name must be rejected at assembler expansion time.
//
// `sort::assemble` dedups `command` + `reply` + `output` items by protocol
// name and errors with `AssemblyError::DuplicateProtocolName { name }` when
// two different kinds share the same name. The error surfaces at the
// `ankyra_config!` expansion site via `abort!`.
#![allow(non_camel_case_types)]

use ankyra_macros::{ankyra_config, ankyra_provider, klipper_command, klipper_reply};

#[klipper_command]
fn ping(_ctx: &mut ()) {}

#[klipper_reply]
pub struct ping {
    pub seq: u32,
}

ankyra_provider! {
    name: COLLIDING_PROVIDER,
    commands: [ping],
    replies: [ping],
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
