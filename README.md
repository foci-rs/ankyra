# ankyra

Klipper MCU protocol framework for Rust. `no_std`-compatible, cross-crate
provider composition, proc-macro driven.

## Status

v0.1.0 — not yet released. See the commit history for current state.

## Quickstart

Library crate publishing a provider:

```rust
use ankyra::prelude::*;

pub trait ClockView {
    fn now(&self) -> u32;
}

#[klipper_reply]
pub struct ClockReply {
    pub clock: u32,
}

#[klipper_command]
fn get_clock(ctx: &mut dyn ClockView) {
    ::ankyra::klipper_reply!(ClockReply, clock: u32 = ctx.now());
}

ankyra_provider! {
    name: CLOCK_PROVIDER,
    commands: [get_clock],
    replies: [ClockReply],
}
```

Firmware crate aggregating providers:

```rust
ankyra::ankyra_config! {
    transport = crate::TRANSPORT_OUTPUT: crate::MyTransportOutput,
    context = &'ctx mut MyState,
    providers = [my_library::CLOCK_PROVIDER],
    static_strings = [],
}
```

See
[`examples/clock_lib/src/lib.rs`](examples/clock_lib/src/lib.rs) and
[`examples/clock_firmware/src/main.rs`](examples/clock_firmware/src/main.rs)
for a complete runnable cross-crate example.

## Crates

- `ankyra` — runtime: descriptor types, transport, encoding, buffers,
  provider/sender traits, built-in `Shutdown` reply.
- `ankyra-macros` — item-definition proc-macros (`#[klipper_command]`,
  `#[klipper_reply]`, `#[klipper_output]`, `#[klipper_constant]`,
  `klipper_enumeration!`, `klipper_static_string!`, `klipper_shutdown!`,
  and the framework macros `ankyra_provider!`, `ankyra_reexport_provider!`,
  `ankyra_config!`).
- `ankyra-assemble` — terminal proc-macro that consumes reified
  protocol-item tokens and emits the generated firmware config module
  (dispatch table, senders, data dictionary, `Transport` binding).

## Relation to anchor

Ankyra is a ground-up rewrite of the
[anchor](https://github.com/Annex-engineering/anchor) crate. The upstream
`anchor` line has been archived, so ankyra does not attempt compatibility
shims or migration layers. The two projects share the Klipper protocol as
a target but nothing else.

Key departures from anchor:

- no `build.rs` source-walking
- no `crate::_anchor_config` coupling
- no implicit call-site reply/output definitions
- explicit providers as the unit of protocol composition
- source-level firmware aggregation via a macro, not a build script

## License

Licensed under either of

- Apache License, Version 2.0
- MIT license

at your option.
