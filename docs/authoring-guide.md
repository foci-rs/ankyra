# Ankyra Authoring Guide

This document shows the two ways of registering `#[klipper_*]` items
with an `ankyra_provider!`.

## Items at the crate root — bare idents

```rust
#[klipper_command]
pub fn get_clock(ctx: &mut dyn ClockCtxView) { /* ... */ }

#[klipper_reply]
pub struct Pong { pub seq: u32 }

ankyra_provider! {
    name: CORE_PROVIDER,
    commands: [get_clock],
    replies:  [Pong],
}
```

## Items in submodules — `crate::…`-prefixed paths

```rust
pub mod klipper_mod {
    #[klipper_command]
    pub fn get_clock(ctx: &mut dyn ClockCtxView) { /* ... */ }

    #[klipper_reply]
    pub struct Pong { pub seq: u32 }
}

ankyra_provider! {
    name: CORE_PROVIDER,
    commands: [crate::klipper_mod::get_clock],
    replies:  [crate::klipper_mod::Pong],
}
```

Both forms can be mixed in a single provider:

```rust
ankyra_provider! {
    name: CORE_PROVIDER,
    commands: [crate::klipper_mod::get_clock, emergency_stop],
    replies:  [crate::klipper_mod::Pong],
}
```

## Constraints on item paths

- Bare idents (`foo`) are equivalent to `crate::foo` (item lives at the
  defining crate's root).
- Multi-segment paths must start with `crate::`. Non-`crate::`-prefixed
  paths (`other_crate::foo`, `::foo::bar`) are rejected with a
  parse-time error; cross-crate providers must go through
  `ankyra_reexport_provider!` as in v0.1.
- Generic arguments / turbofish (`foo::<T>`) are rejected — item
  references are plain paths, not expressions.
- Two items in the same provider list cannot share a leaf ident
  (e.g. `[crate::a::foo, crate::b::foo]`) because they would produce
  duplicate `#[macro_export]` carrier macros and duplicate protocol
  names on the wire.

## Migration from v0.1

Projects that worked around the v0.1 crate-root restriction by
`pub use crate::submod::*` re-exports at the crate root can drop the
re-exports and write the path directly in the provider entry.

**v0.1 workaround:**

```rust
// at crate root:
mod klipper_mod { pub use crate::klipper_mod_inner::*; }
mod klipper_mod_inner {
    #[klipper_command] pub fn get_clock(/* ... */) { /* ... */ }
}
pub use klipper_mod_inner::get_clock;

ankyra_provider! { commands: [get_clock] }
```

**v0.2 equivalent:**

```rust
pub mod klipper_mod {
    #[klipper_command] pub fn get_clock(/* ... */) { /* ... */ }
}

ankyra_provider! { commands: [crate::klipper_mod::get_clock] }
```
