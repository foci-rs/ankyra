// Verify that `#[klipper_reply]` rejects type parameters with a clear
// diagnostic — the assembler has no way to substitute a concrete type for
// `T` when emitting `impl SendReply<Foo<?>> for Sender`, so only lifetime
// parameters are allowed.

use ankyra_macros::klipper_reply;

#[klipper_reply]
pub struct Foo<T> {
    pub value: T,
}

fn main() {}
