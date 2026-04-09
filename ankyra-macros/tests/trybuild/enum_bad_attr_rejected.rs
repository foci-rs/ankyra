use ankyra_macros::klipper_enumeration;

// The header `(...)` only accepts `name = "..."` and `rename_all = "..."`.
// Any other key is rejected with a span-pointed parse error.
klipper_enumeration! {
    pub enum Bad(unknown_attr = "nope") {
        Alpha,
    }
}

fn main() {}
