use ankyra::ScratchOutput;
use ankyra::SendOutput;
use ankyra::encoding::Writable;
use ankyra_macros::{klipper_command, klipper_output};

#[klipper_output]
pub struct Tick {
    pub count: u32,
}

pub struct State;

#[klipper_command]
fn tick(_ctx: &mut State, count: u32) {
    ::ankyra::klipper_output!(Tick, count: u32 = count);
}

struct MockSender {
    last: Option<u32>,
}

impl SendOutput<Tick> for MockSender {
    fn send(&mut self, payload: Tick) {
        self.last = Some(payload.count);
    }
}

fn main() {
    // Encode a frame carrying count=7.
    let mut out = ScratchOutput::<8>::new();
    <u32 as Writable>::write(&7u32, &mut out);
    let bytes = out.result();
    let mut cursor: &[u8] = bytes;

    let mut ctx = State;
    let mut sender = MockSender { last: None };
    __ankyra_dispatch_tick::<MockSender>(&mut cursor, &mut ctx, &mut sender).unwrap();
    assert_eq!(sender.last, Some(7));
}
