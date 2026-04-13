use ankyra::SendReply;
use ankyra_macros::klipper_reply;

#[klipper_reply]
pub struct PingReply {
    pub seq: u32,
}

struct MockSender {
    last: Option<u32>,
}

impl SendReply<PingReply> for MockSender {
    fn send(&mut self, payload: PingReply) {
        self.last = Some(payload.seq);
    }
}

/// Emitted from a plain function (no `#[klipper_command]` wrapper). The
/// sender is supplied explicitly via `klipper_reply_from!`.
fn emit_ping(sender: &mut MockSender, seq: u32) {
    ::ankyra::klipper_reply_from!(sender, PingReply, seq: u32 = seq);
}

fn main() {
    let mut sender = MockSender { last: None };
    emit_ping(&mut sender, 42);
    assert_eq!(sender.last, Some(42));
}
