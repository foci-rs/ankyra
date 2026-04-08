use ankyra_macros::klipper_reply;

#[klipper_reply]
pub struct PingReply {
    pub seq: u32,
}

fn not_a_handler() {
    // `::ankyra::klipper_reply!` references the `__ankyra_sender` binding
    // that is only introduced inside a `#[klipper_command]` handler body.
    // Outside that body, name resolution fails at the reference.
    ::ankyra::klipper_reply!(PingReply, seq: u32 = 42);
}

fn main() {
    not_a_handler();
}
