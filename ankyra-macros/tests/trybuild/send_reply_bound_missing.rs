// Task 13 diagnostic fixture: a `#[klipper_command]` handler emits a reply
// whose struct is NOT listed in any aggregated provider's `replies`. The
// body-scan on `#[klipper_command]` records the
// `S: SendReply<NotAggregated>` bound on the dispatch wrapper, but the
// assembler only emits `impl SendReply<R> for Sender` for replies listed on
// an aggregated provider. The dispatch call site therefore fails to find
// `Sender: SendReply<NotAggregated>` and errors with E0277.

use ankyra_macros::{ankyra_config, ankyra_provider, klipper_command, klipper_reply};

#[klipper_reply]
pub struct NotAggregated {
    pub value: u32,
}

#[klipper_command]
fn emit_missing(_ctx: &mut ()) {
    ::ankyra::klipper_reply!(NotAggregated, value: u32 = 42);
}

// Aggregate only the command, not the reply struct. The firmware's
// `Sender` therefore never receives `impl SendReply<NotAggregated>` in
// `senders::emit`.
ankyra_provider! {
    name: BAD_PROVIDER,
    commands: [emit_missing],
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
    providers = [crate::BAD_PROVIDER],
    static_strings = [],
}

fn main() {}
