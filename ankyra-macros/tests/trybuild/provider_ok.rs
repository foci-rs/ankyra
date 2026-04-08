use ankyra_macros::{ankyra_provider, klipper_command, klipper_reply};

#[klipper_command]
fn emergency_stop(_ctx: &mut ()) {}

#[klipper_reply]
pub struct PingReply {
    pub seq: u32,
}

ankyra_provider! {
    name: CORE_PROVIDER,
    commands: [emergency_stop],
    replies: [PingReply],
}

fn main() {
    let p = CORE_PROVIDER;
    assert_eq!(p.messages().len(), 1);
    assert_eq!(p.replies().len(), 1);
    assert_eq!(p.outputs().len(), 0);
    assert_eq!(p.definitions().len(), 0);
}
