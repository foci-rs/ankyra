//! Positive: a PascalCase `#[klipper_reply]` struct ident is auto-converted
//! to snake_case on the wire. The user-facing Rust type keeps its PascalCase
//! spelling (idiomatic Rust); only the descriptor's protocol name and the
//! synthesized message format adopt the snake_case form.

use ankyra::descriptor::ReplyDescriptor;
use ankyra_macros::klipper_reply;

#[klipper_reply]
pub struct TrsyncState {
    pub seq: u32,
    pub value: i16,
}

fn main() {
    let desc: ReplyDescriptor = __ankyra_descriptor_TrsyncState();
    assert_eq!(desc.protocol_name(), "trsync_state");
    assert_eq!(desc.message_format(), "trsync_state seq=%u value=%hi");
}
