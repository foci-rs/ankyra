use ankyra_macros::klipper_reply;

#[klipper_reply]
pub struct PingReply {
    pub seq: u32,
}

fn main() {
    // Missing the mandatory sender expression and the comma that
    // separates it from the reply path. `klipper_reply_from!` requires
    // `(sender_expr, Path, fields...)`; invoking it with just a single
    // expression triggers the parser's "no comma after first expr" arm
    // which emits a helpful usage message.
    ::ankyra::klipper_reply_from!(PingReply);
}
