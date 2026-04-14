//! Negative fixture: `ankyra_config! { app = 42 }` must be rejected
//! because the dictionary trailer field requires a `&'static str`.
//!
//! The type constraint is enforced at the emitted
//! `pub const __ANKYRA_META_APP: &'static str = <expr>;` binding in
//! `ankyra-assemble/src/dictionary.rs::emit`, which cites the user's
//! span rather than the `concatcp!` internals.

use ankyra_macros::{ankyra_config, ankyra_provider, klipper_command};

#[klipper_command]
fn emergency_stop(_ctx: &mut ()) {}

ankyra_provider! {
    name: CORE_PROVIDER,
    commands: [emergency_stop],
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
    app = 42,
}

fn main() {}
