//! Example provider library demonstrating cross-crate aggregation.
//!
//! This crate defines a [`ProviderRef`] named
//! [`CLOCK_PROVIDER`] that a firmware consumer crate can aggregate via
//! `ankyra_config! { providers = [clock_lib::CLOCK_PROVIDER], ... }`.
//!
//! The provider contributes:
//!
//! * [`ClockReply`] — a reply struct carrying a single `u32` clock tick.
//! * [`get_clock`] — a `#[klipper_command]` handler that reads the current
//!   tick via [`ClockCtxView`] and sends it back as a [`ClockReply`].
//!
//! [`ProviderRef`]: ankyra::provider::ProviderRef
//!
//! [`ClockCtxView`] is the trait the firmware's context type must implement
//! so the handler can read the current tick without depending on the
//! firmware's concrete state type.
//!
//! # Verification goals
//!
//! This crate plus `clock_firmware` demonstrate end-to-end cross-crate
//! aggregation:
//!
//! * `ankyra_provider! CLOCK_PROVIDER` publishes both
//!   `pub const CLOCK_PROVIDER: ProviderRef` and the
//!   `#[macro_export] __ankyra_provider_CLOCK_PROVIDER!` companion at this
//!   crate's root.
//! * The firmware's `ankyra_config! { providers = [clock_lib::CLOCK_PROVIDER] }`
//!   rewrites the path to `clock_lib::__ankyra_provider_CLOCK_PROVIDER` via
//!   [`provider_path_to_companion`]. That companion resolves at `clock_lib`'s
//!   crate root because `#[macro_export]` hoists declarative macros there.
//! * The firmware's emitted `Sender` type receives an
//!   `impl SendReply<ClockReply> for Sender`, produced by the assembler's
//!   path reconstruction from the carrier's `__ankyra_descriptor_ClockReply`
//!   prefix.
//!
//! [`provider_path_to_companion`]: https://docs.rs/ankyra-macros
#![no_std]

use ankyra::prelude::*;

/// View-trait a firmware must implement so `get_clock` can read its tick
/// without depending on the firmware's concrete state type.
pub trait ClockCtxView {
    /// Return the current clock tick.
    fn now(&self) -> u32;
}

// Blanket impl for `&mut T` so that when a firmware declares its
// `Config::Context<'c>` as `&'c mut State`, the dispatcher's
// `&mut &'c mut State` still coerces to `&mut dyn ClockCtxView`. Without
// this impl, rustc would demand `&'c mut State: ClockCtxView` at the
// coercion site and fail — firmware authors would then need to add the
// blanket themselves in every crate. Mirrors ankyra's own
// `impl ShutdownState for &mut T where T: ShutdownState`.
impl<T: ClockCtxView + ?Sized> ClockCtxView for &mut T {
    fn now(&self) -> u32 {
        (**self).now()
    }
}

/// Reply payload for [`get_clock`].
#[klipper_reply]
pub struct ClockReply {
    /// Tick value the firmware reported at the moment the command was
    /// dispatched.
    pub clock: u32,
}

/// Return the firmware's current tick.
///
/// Sends a [`ClockReply`] whose `clock` field is read through
/// [`ClockCtxView::now`]. The body-scan in `#[klipper_command]` records the
/// `S: SendReply<ClockReply>` sender bound automatically so the dispatch
/// wrapper requires it at type-check time.
#[klipper_command]
pub fn get_clock(ctx: &mut dyn ClockCtxView) {
    ::ankyra::klipper_reply!(ClockReply, clock: u32 = ctx.now());
}

ankyra_provider! {
    name: CLOCK_PROVIDER,
    commands: [get_clock],
    replies: [ClockReply],
}
