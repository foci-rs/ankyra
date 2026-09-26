//! Positive: a 58-byte worst-case body (11 x `u32` + 1 x `u16`). The reply's
//! id here is 3, which encodes in 1 byte, so the frame payload is exactly 59.

use ankyra_macros::{ankyra_config, ankyra_provider, klipper_command, klipper_reply};

#[klipper_command]
fn noop(_ctx: &mut ()) {}

#[klipper_reply]
pub struct ExactReply {
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
    pub w: u16,
}

ankyra_provider! {
    name: CORE_PROVIDER,
    commands: [noop],
    replies: [ExactReply],
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
