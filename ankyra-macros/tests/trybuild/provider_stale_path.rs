use ankyra_macros::{ankyra_config, ankyra_provider};

// The provider references `wrong_module::get_clock` but no such module exists.
// The assembler reconstructs a dispatch path from the stored prefix, so
// `ankyra_config!` expansion emits `crate::wrong_module::get_clock(…)` which
// rustc cannot resolve — E0433 fires at the config expansion site.
ankyra_provider! {
    name: STALE,
    commands: [crate::wrong_module::get_clock],
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
    providers = [crate::STALE],
    static_strings = [],
}

fn main() {}
