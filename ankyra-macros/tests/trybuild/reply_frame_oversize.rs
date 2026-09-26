//! Negative: thirteen `u32` fields are a 65-byte worst-case body, past the
//! 59-byte frame payload before the reply id (3) is even counted.

use ankyra_macros::{ankyra_config, ankyra_provider, klipper_command, klipper_reply};

#[klipper_command]
fn noop(_ctx: &mut ()) {}

#[klipper_reply]
pub struct OversizeReply {
    pub v0: u32,
    pub v1: u32,
    pub v2: u32,
    pub v3: u32,
    pub v4: u32,
    pub v5: u32,
    pub v6: u32,
    pub v7: u32,
    pub v8: u32,
    pub v9: u32,
    pub v10: u32,
    pub v11: u32,
    pub v12: u32,
}

ankyra_provider! {
    name: CORE_PROVIDER,
    commands: [noop],
    replies: [OversizeReply],
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
    providers = [crate::CORE_PROVIDER],
    static_strings = [],
}

fn main() {}
