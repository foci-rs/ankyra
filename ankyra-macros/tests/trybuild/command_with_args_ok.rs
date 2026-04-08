use ankyra::ScratchOutput;
use ankyra::encoding::Writable;
use ankyra_macros::klipper_command;

pub struct State;

#[klipper_command]
fn set_timer(_ctx: &mut State, oid: u8, ticks: u32) {
    let _ = (oid, ticks);
}

fn main() {
    // Encode a frame: oid=42, ticks=1_234_567
    let mut out = ScratchOutput::<16>::new();
    <u8 as Writable>::write(&42u8, &mut out);
    <u32 as Writable>::write(&1_234_567u32, &mut out);
    let bytes = out.result();
    let mut cursor: &[u8] = bytes;

    let mut ctx = State;
    let mut sender = ();
    __ankyra_dispatch_set_timer::<()>(&mut cursor, &mut ctx, &mut sender).unwrap();
}
