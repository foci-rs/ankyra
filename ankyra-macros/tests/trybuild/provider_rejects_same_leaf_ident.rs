use ankyra_macros::{ankyra_provider, klipper_command};

pub mod a {
    use ankyra_macros::klipper_command;
    #[klipper_command]
    pub fn foo(_c: &mut ()) {}
}
pub mod b {
    // Intentionally does NOT carry #[klipper_command]; the provider
    // error should fire before rustc tries to resolve the path.
    pub fn foo(_c: &mut ()) {}
}

ankyra_provider! {
    name: COLLIDE,
    commands: [crate::a::foo, crate::b::foo],
}

fn main() {}
