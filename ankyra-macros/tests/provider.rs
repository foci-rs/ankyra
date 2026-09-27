//! Integration test for `ankyra_provider!`.
//!
//! The trybuild fixture `provider_ok.rs` proves the emitted code compiles;
//! this file proves the user-facing `ProviderRef` const actually reports
//! the right counts at runtime.

use ankyra_macros::{ankyra_provider, klipper_command, klipper_reply};

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

#[test]
fn provider_exposes_messages_and_replies() {
    assert_eq!(CORE_PROVIDER.messages().len(), 1);
    assert_eq!(CORE_PROVIDER.replies().len(), 1);
    assert_eq!(CORE_PROVIDER.outputs().len(), 0);
    assert_eq!(CORE_PROVIDER.definitions().len(), 0);

    let cmd = CORE_PROVIDER.messages()[0];
    assert_eq!(cmd.protocol_name(), "emergency_stop");
    assert_eq!(cmd.message_format(), "emergency_stop");

    let reply = CORE_PROVIDER.replies()[0];
    assert_eq!(reply.protocol_name(), "ping_reply");
    assert_eq!(reply.message_format(), "ping_reply seq=%u");
}
