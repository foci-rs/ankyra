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

// The `Config::Context<'c>` GAT requires `ShutdownState`. `()` already
// satisfies it; `State` does not, so we add the minimal impl here.
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

/// D3 verification: `handle_identify` zlib-compresses `DICT_BYTES`
/// before streaming to the host. Stand up a scratch buffer the same
/// way (sized with the public `max_compressed_size` bound) and confirm
/// the output is a valid zlib stream — first byte is the 0x78 magic,
/// `(cmf << 8 | flg) % 31 == 0`.
const SCRATCH_LEN: usize =
    ankyra::dictionary::max_compressed_size(_ankyra_config::DICT_BYTES.len());

fn main() {
    // Prove the transport is resolvable and of the expected type. The
    // assembler emits `Config` inside `crate::_ankyra_config`; we lean on
    // inference (`Transport<_>`) rather than naming it explicitly so the
    // main fn does not depend on the assembler's internal module layout.
    let _: &ankyra::transport::Transport<_> = &KLIPPER_TRANSPORT;

    // D1 verification: the assembled dictionary must carry cross-crate
    // user-item format strings. `ClockReply` is PascalCase in Rust but
    // auto-converts to `clock_reply` on the wire via
    // `ankyra_macros::shared::pascal_to_snake`, so the hoisted format
    // string is `clock_reply clock=%u`.
    let dict = core::str::from_utf8(_ankyra_config::DICT_BYTES).expect("dictionary is valid UTF-8");
    assert!(
        dict.contains("clock_reply clock=%u"),
        "dictionary missing cross-crate reply format: {dict}"
    );
    assert!(
        dict.contains("get_clock"),
        "dictionary missing cross-crate command name: {dict}"
    );

    let mut scratch = [0u8; SCRATCH_LEN];
    let n = ankyra::dictionary::compress_dict_to(_ankyra_config::DICT_BYTES, &mut scratch)
        .expect("compressed dictionary fits in max_compressed_size buffer");
    assert!(n >= 6, "zlib stream too short to be valid: got {n} bytes");
    assert_eq!(
        scratch[0], 0x78,
        "compressed dictionary missing zlib magic 0x78; got 0x{:02x}",
        scratch[0]
    );
    let header = u16::from_be_bytes([scratch[0], scratch[1]]);
    assert_eq!(header % 31, 0, "zlib FCHECK invalid");

    println!(
        "firmware aggregated clock_lib successfully; compressed dictionary: {n} bytes ({} uncompressed)",
        _ankyra_config::DICT_BYTES.len()
    );
}
