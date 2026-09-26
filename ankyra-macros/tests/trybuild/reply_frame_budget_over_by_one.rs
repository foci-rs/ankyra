//! Negative twin of `reply_frame_budget_exact_ok`: a 59-byte worst-case body
//! (11 x `u32` + 2 x `u8`) with id 3 (1 byte) is a 60-byte payload.

use ankyra_macros::{ankyra_config, ankyra_provider, klipper_command, klipper_reply};

#[klipper_command]
fn noop(_ctx: &mut ()) {}

#[klipper_reply]
pub struct OverByOneReply {
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
    pub w0: u8,
    pub w1: u8,
}

ankyra_provider! {
    name: CORE_PROVIDER,
    commands: [noop],
    replies: [OverByOneReply],
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
