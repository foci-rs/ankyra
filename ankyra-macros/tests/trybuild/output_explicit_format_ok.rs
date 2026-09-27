use ankyra::ScratchOutput;
use ankyra::encoding::Writable;
use ankyra_macros::klipper_output;

#[klipper_output(format = "hello v=%u s=%.*s")]
pub struct Hello<'a> {
    pub v: u32,
    pub s: &'a str,
}

fn main() {
    let h = Hello { v: 1, s: "hi" };
    let mut out = ScratchOutput::<32>::new();
    <Hello as Writable>::write(&h, &mut out);
    let _ = out.result();

    let desc = __ankyra_descriptor_Hello();
    assert_eq!(desc.protocol_name(), "hello");
    assert_eq!(desc.message_format(), "hello v=%u s=%.*s");
}
