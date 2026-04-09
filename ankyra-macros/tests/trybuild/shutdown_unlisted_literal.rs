use ankyra_macros::{ankyra_config, ankyra_provider, klipper_command};

// The shutdown literal `"different"` is not listed in the firmware's
// `static_strings = [...]` entry, so the FNV-1a-hashed path emitted by
// `klipper_shutdown!` fails to resolve at the call site.
#[klipper_command]
fn panic_now<S>(_ctx: &mut (), clock: u32)
where
    S: ankyra::SendReply<ankyra::Shutdown>,
{
    ::ankyra::klipper_shutdown!("different", clock);
}

ankyra_provider! {
    name: CORE_PROVIDER,
    commands: [panic_now],
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
    static_strings = ["registered"],
}

fn main() {}
