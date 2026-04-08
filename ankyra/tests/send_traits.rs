use ankyra::send::{SendOutput, SendReply};

pub struct LocalReply;
pub struct LocalOutput;
pub struct LocalSender;

impl SendReply<LocalReply> for LocalSender {
    fn send(&mut self, _payload: LocalReply) {}
}

impl SendOutput<LocalOutput> for LocalSender {
    fn send(&mut self, _payload: LocalOutput) {}
}

#[test]
fn send_reply_is_callable() {
    let mut s = LocalSender;
    <LocalSender as SendReply<LocalReply>>::send(&mut s, LocalReply);
}

#[test]
fn send_output_is_callable() {
    let mut s = LocalSender;
    <LocalSender as SendOutput<LocalOutput>>::send(&mut s, LocalOutput);
}
