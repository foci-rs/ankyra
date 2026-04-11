use ankyra_macros::ankyra_provider;

ankyra_provider! {
    name: BAD,
    commands: [foo::<T>],
}

fn main() {}
