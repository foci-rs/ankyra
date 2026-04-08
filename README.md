# ankyra

Ankyra is a Klipper MCU communication framework for Rust, designed around
explicit protocol-item definitions, explicit provider exports, and source-level
firmware aggregation.

## Relation to anchor

Ankyra is a ground-up rewrite of the [anchor](https://github.com/Annex-engineering/anchor)
crate. The upstream `anchor` line has been archived, so ankyra does not attempt
compatibility shims or migration layers. The two projects share the Klipper
protocol as a target but nothing else.

Key departures from anchor:

- no `build.rs` source-walking
- no `crate::_anchor_config` coupling
- no implicit call-site reply/output definitions
- explicit providers as the unit of protocol composition
- source-level firmware aggregation via a macro, not a build script

## Crates

- `ankyra` — runtime, descriptor types, provider contracts, encoding helpers.
- `ankyra-macros` — item-definition proc-macros (`#[klipper_command]`,
  `#[klipper_reply]`, `#[klipper_output]`, `#[klipper_constant]`,
  `klipper_enumeration!`, `klipper_static_string!`, `klipper_shutdown!`,
  `klipper_reply!`, `klipper_output!`, plus framework macros `ankyra_provider!`
  and `ankyra_config!`).
- `ankyra-assemble` — proc-macro terminal that consumes reified protocol-item
  tokens and emits the generated firmware config module.

## Status

Pre-0.1 — the design spec and implementation plan live under
`docs/superpowers/`. The code tree is skeletal until the plan is executed.
