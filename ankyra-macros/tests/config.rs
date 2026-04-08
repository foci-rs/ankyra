//! Integration test for `ankyra_config!`.
//!
//! The shim expands into the CPS fold, which walks the single-provider
//! remaining list, accumulates carrier tuples, and hands them to
//! `__ankyra_assemble!`. The terminal emission (Task 12) produces a
//! `mod _ankyra_config { ... }` tree containing the data dictionary, the
//! `IdentifyResponse` reply + `handle_identify` dispatcher, the `Sender`
//! type with its `SendReply` / `SendOutput` impls, the `Config` trait
//! impl, the `KLIPPER_TRANSPORT` transport, and a `static_strings`
//! submodule with one `pub const __ANKYRA_SS_<hash>: u16` per listed
//! literal.
//!
//! This test proves:
//!
//! 1. The pipeline compiles end-to-end from `ankyra_config!` through the
//!    assembler.
//! 2. `KLIPPER_TRANSPORT` is visible at the test crate's root as a real
//!    `Transport<Config>` value, matching the firmware ergonomics spec.
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
    // Task 12 promotes `KLIPPER_TRANSPORT` from the Task 10 unit
    // placeholder to a real `Transport<Config>` value. Reference it to
    // force a compile-time check that the name resolves at the firmware
    // crate's root and carries the right type — equivalent in spirit to
    // the `let () = KLIPPER_TRANSPORT` check used under the Task 10 stub.
    let _: &ankyra::transport::Transport<_> = &KLIPPER_TRANSPORT;
}

#[test]
fn listed_static_string_resolves() {
    let id: u16 = ankyra::klipper_static_string!("test probe");
    // ID 0 is reserved for the synthesized `identify_response` reply; any
    // user-listed static string gets a positive id from the assembler's
    // monotonically-increasing counter.
    assert!(id > 0, "listed static string must get a non-zero id");
}
