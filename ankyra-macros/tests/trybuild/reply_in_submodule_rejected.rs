// Exercises the v0.1 crate-root invariant for `#[klipper_reply]`.
//
// The macro emits a `pub use crate::__ankyra_descriptor_<Name>` sniff at the
// expansion scope. Invoked at the crate root that re-export is a redundant
// alias; invoked inside a submodule (as here) `crate::__ankyra_descriptor_X`
// does not resolve (the descriptor fn lives at `crate::inner::...`) and
// rustc reports `E0432 unresolved import`. That error is the diagnostic the
// user sees when they violate the invariant.
//
// The invariant will be lifted in v0.2. Until then, define `#[klipper_*]`
// items at the defining crate's root and re-export them from submodules if
// desired.

mod inner {
    use ankyra_macros::klipper_reply;

    #[klipper_reply]
    pub struct SubReply {
        pub seq: u32,
    }
}

fn main() {
    let _id: u16 = 0;
}
