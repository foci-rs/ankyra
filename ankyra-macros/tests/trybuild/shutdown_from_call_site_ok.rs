//! Positive: `klipper_shutdown_from!` invoked from a plain `fn` outside a
//! `#[klipper_command]` handler. The `_ankyra_config::static_strings`
//! module is provided by a full `ankyra_config!` setup so the FNV-1a
//! hashed path emitted by the macro resolves.
//!
//! The sender is a bespoke mock implementing `SendReply<Shutdown>`; the
//! macro drives the send via the single-evaluation shim that binds the
//! expression to `__ankyra_sender`.

use ankyra::SendReply;
use ankyra::Shutdown;
use ankyra_macros::{ankyra_config, ankyra_provider, klipper_command};

// A no-op command just to satisfy `ankyra_provider!` — the firmware
// assembler requires at least one registered item. The interesting call
// happens in `raise_shutdown` below. Use `()` as the context so we can
// rely on ankyra's blanket `ShutdownState` impl for `()`.
#[klipper_command]
fn noop(_ctx: &mut ()) {}

ankyra_provider! {
    name: CORE_PROVIDER,
    commands: [noop],
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
    static_strings = ["hardware fault"],
}

struct MockSender {
    last_clock: Option<u32>,
}

impl SendReply<Shutdown> for MockSender {
    fn send(&mut self, payload: Shutdown) {
        self.last_clock = Some(payload.clock);
    }
}

/// Emitted from a plain function (no `#[klipper_command]` wrapper). The
/// sender is supplied explicitly via `klipper_shutdown_from!`.
fn raise_shutdown(sender: &mut MockSender, clock: u32) {
    ::ankyra::klipper_shutdown_from!(sender, "hardware fault", clock);
}

fn main() {
    let mut sender = MockSender { last_clock: None };
    raise_shutdown(&mut sender, 0xDEAD_BEEF);
    assert_eq!(sender.last_clock, Some(0xDEAD_BEEF));
}
