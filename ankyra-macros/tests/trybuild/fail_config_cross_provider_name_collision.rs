use ankyra_macros::{ankyra_config, ankyra_provider};

pub mod a {
    use ankyra_macros::klipper_command;
    #[klipper_command]
    pub fn get_clock(_c: &mut ()) {}
}
pub mod b {
    // Intentionally does NOT carry #[klipper_command]; the assembler
    // duplicate-name check fires before rustc tries to resolve the path.
    pub fn get_clock(_c: &mut ()) {}
}

ankyra_provider! {
    name: P1,
    commands: [crate::a::get_clock],
}
ankyra_provider! {
    name: P2,
    commands: [crate::b::get_clock],
}

pub struct BufferTransportOutput;
pub const TRANSPORT_OUTPUT: BufferTransportOutput = BufferTransportOutput;

impl ankyra::TransportOutput for BufferTransportOutput {
    type Output = ankyra::ScratchOutput<64>;
    fn output(&self, f: impl FnOnce(&mut Self::Output)) {
        let mut o = ankyra::ScratchOutput::<64>::new();
        f(&mut o);
    }
}

ankyra_config! {
    transport = crate::TRANSPORT_OUTPUT: crate::BufferTransportOutput,
    context = &'ctx mut (),
    providers = [crate::P1, crate::P2],
    static_strings = [],
}

fn main() {}
