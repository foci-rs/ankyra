use ankyra_macros::{ankyra_provider, klipper_command};

#[klipper_command]
fn a(_ctx: &mut ()) {}

ankyra_provider! {
    name: BAD,
    wut_is_this: [a],
}

fn main() {}
