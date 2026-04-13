use ankyra::ScratchOutput;
use ankyra::encoding::Writable;
use ankyra::reply::ReplyPayload;
use ankyra_macros::klipper_reply;

#[klipper_reply]
pub struct PingReply {
    pub seq: u32,
    pub value: i16,
}

fn _assert_reply_payload<T: ReplyPayload>() {}

fn main() {
    // Marker trait is implemented.
    _assert_reply_payload::<PingReply>();

    // Writable impl compiles and runs.
    let p = PingReply { seq: 1, value: -2 };
    let mut out = ScratchOutput::<16>::new();
    <PingReply as Writable>::write(&p, &mut out);
    let _ = out.result();

    // Descriptor fn exists at module scope and returns a ReplyDescriptor.
    // The PascalCase struct ident `PingReply` produces the snake_case wire
    // name `ping_reply`; the descriptor fn still uses the raw ident suffix.
    let desc = __ankyra_descriptor_PingReply();
    assert_eq!(desc.protocol_name(), "ping_reply");
    assert_eq!(desc.message_format(), "ping_reply seq=%u value=%hi");
}
