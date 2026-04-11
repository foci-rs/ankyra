use ankyra_macros::ankyra_provider;

ankyra_provider! {
    name: BAD,
    commands: [other_crate::foo],
}

fn main() {}
