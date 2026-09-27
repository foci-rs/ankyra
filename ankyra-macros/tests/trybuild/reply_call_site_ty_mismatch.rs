use ankyra_macros::{klipper_command, klipper_reply};

#[klipper_reply]
pub struct PingReply {
    pub seq: u32,
}

pub struct State;

#[klipper_command]
fn ping(_ctx: &mut State) {
    ::ankyra::klipper_reply!(PingReply, seq: u8 = 7);
}

fn main() {}
