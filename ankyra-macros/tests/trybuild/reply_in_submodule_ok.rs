//! Positive: #[klipper_reply] inside a submodule compiles.

pub mod api {
    use ankyra_macros::klipper_reply;
    #[klipper_reply]
    pub struct Pong {
        pub seq: u32,
    }
}

fn main() {}
