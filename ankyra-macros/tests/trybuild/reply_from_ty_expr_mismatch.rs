use ankyra::SendReply;
use ankyra_macros::klipper_reply;

#[klipper_reply]
pub struct PingReply {
    pub seq: u32,
}

fn emit_ping<S: SendReply<PingReply>>(sender: &mut S, seq: u8) {
    ::ankyra::klipper_reply_from!(sender, PingReply, seq: u32 = seq);
}

fn main() {}
