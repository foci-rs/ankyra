use ankyra::ScratchOutput;
use ankyra::SendReply;
use ankyra::encoding::Writable;
use ankyra_macros::{klipper_command, klipper_reply};

#[klipper_reply]
pub struct PingReply {
    pub seq: u32,
}

pub struct State;

#[klipper_command]
fn ping(_ctx: &mut State, seq: u32) {
    ::ankyra::klipper_reply!(PingReply, seq: u32 = seq);
}

struct MockSender {
    last: Option<u32>,
}

impl SendReply<PingReply> for MockSender {
    fn send(&mut self, payload: PingReply) {
        self.last = Some(payload.seq);
    }
}

fn main() {
    // Encode a frame carrying seq=42.
    let mut out = ScratchOutput::<8>::new();
    <u32 as Writable>::write(&42u32, &mut out);
    let bytes = out.result();
    let mut cursor: &[u8] = bytes;

    let mut ctx = State;
    let mut sender = MockSender { last: None };
    __ankyra_dispatch_ping::<MockSender>(&mut cursor, &mut ctx, &mut sender).unwrap();
    assert_eq!(sender.last, Some(42));
}
