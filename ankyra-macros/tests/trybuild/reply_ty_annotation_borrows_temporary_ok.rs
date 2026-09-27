use ankyra::SendReply;
use ankyra_macros::klipper_reply;

#[klipper_reply]
pub struct Blob<'a> {
    pub data: &'a [u8],
}

struct MockSender {
    len: usize,
}

impl<'a> SendReply<Blob<'a>> for MockSender {
    fn send(&mut self, payload: Blob<'a>) {
        self.len = payload.data.len();
    }
}

fn make() -> [u8; 3] {
    [1, 2, 3]
}

fn main() {
    let mut sender = MockSender { len: 0 };
    ::ankyra::klipper_reply_from!(&mut sender, Blob, data: &[u8] = &make());
    assert_eq!(sender.len, 3);
}
