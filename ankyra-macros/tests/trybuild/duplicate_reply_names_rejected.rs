use ankyra_macros::klipper_reply;

// Two `#[klipper_reply]` structs sharing an identifier collide at the
// struct name (E0428) and at the `#[macro_export] __ankyra_item_reply_Dup`
// carrier emitted for each — `#[macro_export]` publishes both at crate
// root so a second definition is rejected there too.
#[klipper_reply]
pub struct Dup {
    pub a: u32,
}

#[klipper_reply]
pub struct Dup {
    pub b: u32,
}

fn main() {}
