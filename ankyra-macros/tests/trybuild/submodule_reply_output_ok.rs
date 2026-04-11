use ankyra_macros::ankyra_provider;

pub mod api {
    use ankyra_macros::{klipper_output, klipper_reply};
    #[klipper_reply]
    pub struct Pong {
        pub seq: u32,
    }
    #[klipper_output(format = "tick t=%u")]
    pub struct Tick {
        pub t: u32,
    }
}

ankyra_provider! {
    name: P,
    replies: [crate::api::Pong],
    outputs: [crate::api::Tick],
}

fn main() {}
