use ankyra_macros::{ankyra_provider, klipper_command};

#[klipper_command]
fn emergency_stop(_ctx: &mut ()) {}

ankyra_provider! {
    name: CORE_PROVIDER,
    commands: [emergency_stop, emergency_stop],
}

fn main() {}
