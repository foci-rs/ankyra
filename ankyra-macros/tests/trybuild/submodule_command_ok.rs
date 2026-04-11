//! Positive: submodule command through the full v0.2 provider +
//! assembler pipeline (ProviderPath parsing, wrapped-tuple carrier,
//! submodule-aware sibling-path reconstruction).

use ankyra_macros::{ankyra_config, ankyra_provider, klipper_command};

pub mod klipper_mod {
    use ankyra_macros::klipper_command;
    #[klipper_command]
    pub fn get_clock(_ctx: &mut ()) {}
}

#[klipper_command]
fn emergency_stop(_ctx: &mut ()) {}

ankyra_provider! {
    name: CORE_PROVIDER,
    commands: [crate::klipper_mod::get_clock, emergency_stop],
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
    providers = [crate::CORE_PROVIDER],
    static_strings = [],
}

fn main() {}
