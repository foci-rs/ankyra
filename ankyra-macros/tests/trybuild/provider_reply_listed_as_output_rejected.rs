//! Negative: a provider that lists a reply under `outputs` fails in its own
//! build.

use ankyra_macros::{ankyra_provider, klipper_reply};

#[klipper_reply]
pub struct PingReply {
    pub seq: u32,
}

ankyra_provider! {
    name: MIXED,
    outputs: [PingReply],
}

fn main() {}
