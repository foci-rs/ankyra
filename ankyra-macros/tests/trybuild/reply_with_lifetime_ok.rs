// Verify `#[klipper_reply]` accepts lifetime-parameterized structs and
// that the emitted `Writable`/`ReplyPayload` impls forward the lifetime.
// Uses `klipper_reply_from!` so the fixture stays free of `ankyra_config!`
// — the end-to-end `impl<'a> SendReply<Borrowed<'a>> for Sender` emission
// exercised by the assembler is covered separately by the FOCI binary.

use ankyra::SendReply;
use ankyra::reply::ReplyPayload;
use ankyra_macros::klipper_reply;

#[klipper_reply]
pub struct Borrowed<'a> {
    pub seq: u32,
    pub slice: &'a [u8],
}

fn _assert_reply_payload<T: ReplyPayload>() {}

struct MockSender {
    last_seq: Option<u32>,
    last_len: Option<usize>,
}

impl<'a> SendReply<Borrowed<'a>> for MockSender {
    fn send(&mut self, payload: Borrowed<'a>) {
        self.last_seq = Some(payload.seq);
        self.last_len = Some(payload.slice.len());
    }
}

fn emit(sender: &mut MockSender, seq: u32, slice: &[u8]) {
    ::ankyra::klipper_reply_from!(sender, Borrowed, seq: u32 = seq, slice: &[u8] = slice);
}

fn main() {
    // Lifetime is forwarded to the marker trait and Writable impls via the
    // struct's own generics.
    _assert_reply_payload::<Borrowed<'_>>();

    let mut sender = MockSender {
        last_seq: None,
        last_len: None,
    };
    emit(&mut sender, 7, b"hello");
    assert_eq!(sender.last_seq, Some(7));
    assert_eq!(sender.last_len, Some(5));

    // The descriptor fn is emitted without generics and resolves at module
    // scope — the struct's lifetime is irrelevant to the descriptor data.
    let desc = __ankyra_descriptor_Borrowed();
    assert_eq!(desc.protocol_name(), "borrowed");
    assert_eq!(desc.message_format(), "borrowed seq=%u slice=%*s");
}
