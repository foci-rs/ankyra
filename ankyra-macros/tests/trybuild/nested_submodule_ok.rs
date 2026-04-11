use ankyra_macros::ankyra_provider;

pub mod a {
    pub mod b {
        pub mod c {
            use ankyra_macros::klipper_command;
            #[klipper_command]
            pub fn get_clock(_c: &mut ()) {}
        }
    }
}

ankyra_provider! {
    name: DEEP,
    commands: [crate::a::b::c::get_clock],
}

fn main() {}
