use ankyra_macros::{ankyra_provider, klipper_command};

#[klipper_command]
fn foo(_ctx: &mut ()) {}

ankyra_provider! {
    name: BAD,
    commands: [::foo::bar],
}

fn main() {}
