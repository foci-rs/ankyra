use ankyra::SendReply;
use ankyra_macros::klipper_reply;

#[klipper_reply]
pub struct PingReply {
    pub seq: u32,
}

fn emit_ping<S: SendReply<PingReply>>(sender: &mut S) {
    ::ankyra::klipper_reply_from!(sender, PingReply, seq: impl Copy = 7u32);
}

fn main() {}
