//! Positive / backward-compat: an already-lowercase `#[klipper_reply]`
//! struct ident is preserved verbatim. Existing consumers that wrote
//! `pub struct trsync_state` with `#[allow(non_camel_case_types)]` continue
//! to emit the same wire name after the auto-conversion lands.

#![allow(non_camel_case_types)]

use ankyra::descriptor::ReplyDescriptor;
use ankyra_macros::klipper_reply;

#[klipper_reply]
pub struct stats {
    pub count: u32,
}

fn main() {
    let desc: ReplyDescriptor = __ankyra_descriptor_stats();
    // Ident has no uppercase letters -> returned verbatim.
    assert_eq!(desc.protocol_name(), "stats");
    assert_eq!(desc.message_format(), "stats count=%u");
}
