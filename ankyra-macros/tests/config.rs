//! Integration test for `ankyra_config!`.
//!
//! This test proves:
//!
//! 1. The pipeline compiles end-to-end from `ankyra_config!` through the
//!    assembler.
//! 2. `KLIPPER_TRANSPORT` is visible at the test crate's root as a real
//!    `Transport<Config>` value.
//! 3. A listed `static_strings` literal resolves to a `u16` const through
//!    the `klipper_static_string!` call-site macro.

use ankyra_macros::{ankyra_config, ankyra_provider, klipper_command, klipper_reply};

#[klipper_command]
fn emergency_stop(_ctx: &mut ()) {}

#[klipper_reply]
pub struct PingReply {
    pub seq: u32,
}

ankyra_provider! {
    name: CORE_PROVIDER,
    commands: [emergency_stop],
    replies: [PingReply],
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
    static_strings = ["test probe"],
}

#[test]
fn config_exports_transport() {
    let _: &ankyra::transport::Transport<_> = &KLIPPER_TRANSPORT;
}

#[test]
fn listed_static_string_resolves() {
    let id: u16 = ankyra::klipper_static_string!("test probe");
    assert!(id > 0, "listed static string must get a non-zero id");
}
