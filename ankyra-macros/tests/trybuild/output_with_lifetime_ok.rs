// Mirror of `reply_with_lifetime_ok.rs` for `#[klipper_output]`: verify
// the attribute accepts lifetime-parameterized structs and the emitted
// `Writable` / `OutputPayload` impls propagate the lifetime.

use ankyra::SendOutput;
use ankyra::reply::OutputPayload;
use ankyra_macros::klipper_output;

#[klipper_output]
pub struct Beacon<'a> {
    pub tag: u16,
    pub payload: &'a [u8],
}

fn _assert_output_payload<T: OutputPayload>() {}

struct MockSender {
    last_tag: Option<u16>,
    last_len: Option<usize>,
}

impl<'a> SendOutput<Beacon<'a>> for MockSender {
    fn send(&mut self, payload: Beacon<'a>) {
        self.last_tag = Some(payload.tag);
        self.last_len = Some(payload.payload.len());
    }
}

fn emit(sender: &mut MockSender, tag: u16, payload: &[u8]) {
    ::ankyra::klipper_output_from!(
        sender,
        Beacon,
        tag: u16 = tag,
        payload: &[u8] = payload,
    );
}

fn main() {
    _assert_output_payload::<Beacon<'_>>();
    let mut sender = MockSender {
        last_tag: None,
        last_len: None,
    };
    emit(&mut sender, 42, b"abc");
    assert_eq!(sender.last_tag, Some(42));
    assert_eq!(sender.last_len, Some(3));

    let desc = __ankyra_descriptor_Beacon();
    assert_eq!(desc.protocol_name(), "beacon");
    assert_eq!(desc.message_format(), "beacon tag=%hu payload=%*s");
}
