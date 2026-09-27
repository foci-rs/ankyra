//! Minimal binary consumer that aggregates `clock_lib::CLOCK_PROVIDER`.
//!
//! The sole purpose of this binary is to force the cross-crate aggregation
//! path of `ankyra_config!` to expand, compile, and link. Success means:
//!
//! * `clock_lib::CLOCK_PROVIDER` rewrote to the companion macro at
//!   `clock_lib::__ankyra_provider_CLOCK_PROVIDER` and the companion
//!   resolved because `#[macro_export]` hoists it to `clock_lib`'s root.
//! * The assembler reconstructed `clock_lib::ClockReply` from its carrier
//!   descriptor prefix and emitted a working `impl SendReply<ClockReply>
//!   for Sender` in the firmware's generated module tree.
//! * The `KLIPPER_TRANSPORT` re-export at the firmware crate's root resolves
//!   and has the expected `Transport<Config>` type.

use ankyra::transport::ShutdownState;
use clock_lib::ClockCtxView;

/// Minimal firmware state. The tick value is what `ClockCtxView::now()`
/// returns when the handler dispatches.
pub struct State {
    /// Current clock tick exposed via [`ClockCtxView`].
    pub tick: u32,
}

impl ClockCtxView for State {
    fn now(&self) -> u32 {
        self.tick
    }
}

impl ShutdownState for State {
    fn is_shutdown(&self) -> bool {
        false
    }
}

/// Zero-sized forwarder used as the firmware's [`ankyra::TransportOutput`].
///
/// Real firmware would spill encoded bytes into a USB CDC-ACM ring buffer or
/// UART DMA region; this example discards them because the binary exists
/// only to prove the aggregation compiles and links.
pub struct BufferTransportOutput;
/// Single instance of [`BufferTransportOutput`] passed to the assembler.
pub const TRANSPORT_OUTPUT: BufferTransportOutput = BufferTransportOutput;

impl ankyra::TransportOutput for BufferTransportOutput {
    type Output = ankyra::ScratchOutput<64>;
    fn output(&self, f: impl FnOnce(&mut Self::Output)) {
        let mut o = ankyra::ScratchOutput::<64>::new();
        f(&mut o);
    }
}

ankyra::ankyra_config! {
    transport = crate::TRANSPORT_OUTPUT: crate::BufferTransportOutput,
    context = &'ctx mut State,
    providers = [clock_lib::CLOCK_PROVIDER],
    static_strings = [],
    app = "clock-firmware",
    version = env!("CARGO_PKG_VERSION"),
    build_versions = "",
    license = "MIT OR Apache-2.0",
}

fn main() {
    let _: &ankyra::transport::Transport<_> = &KLIPPER_TRANSPORT;

    let dict = core::str::from_utf8(_ankyra_config::DICT_BYTES).expect("dictionary is valid UTF-8");
    assert!(
        dict.contains("clock_reply clock=%u"),
        "dictionary missing cross-crate reply format: {dict}"
    );
    assert!(
        dict.contains("get_clock"),
        "dictionary missing cross-crate command name: {dict}"
    );

    let compressed = &_ankyra_config::COMPRESSED_DICT;
    let n = compressed.len();
    assert!(n >= 6, "zlib stream too short to be valid: got {n} bytes");
    assert_eq!(
        compressed[0], 0x78,
        "compressed dictionary missing zlib magic 0x78; got 0x{:02x}",
        compressed[0]
    );
    let header = u16::from_be_bytes([compressed[0], compressed[1]]);
    assert_eq!(header % 31, 0, "zlib FCHECK invalid");

    println!(
        "firmware aggregated clock_lib successfully; compressed dictionary: {n} bytes ({} uncompressed)",
        _ankyra_config::DICT_BYTES.len()
    );
}
