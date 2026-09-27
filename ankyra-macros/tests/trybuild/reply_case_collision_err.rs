//! Negative: when two struct idents in the same crate normalise to the
//! same snake_case wire name, the assembler rejects them with a
//! `duplicate protocol name` diagnostic. `FooBar` auto-converts to
//! `foo_bar`, colliding with the verbatim lowercase `foo_bar` struct.

#![allow(non_camel_case_types)]

use ankyra_macros::{ankyra_config, ankyra_provider, klipper_command, klipper_reply};

#[klipper_command]
fn ping(_ctx: &mut ()) {}

#[klipper_reply]
pub struct FooBar {
    pub seq: u32,
}

#[klipper_reply]
pub struct foo_bar {
    pub seq: u32,
}

ankyra_provider! {
    name: COLLIDING_PROVIDER,
    commands: [ping],
    replies: [FooBar, foo_bar],
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
