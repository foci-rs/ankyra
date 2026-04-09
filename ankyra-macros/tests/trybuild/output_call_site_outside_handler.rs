use ankyra_macros::klipper_output;

#[klipper_output]
pub struct DebugPrint {
    pub value: u32,
}

fn not_a_handler() {
    // `::ankyra::klipper_output!` references the `__ankyra_sender` binding
    // that is only introduced inside a `#[klipper_command]` handler body.
    // Outside that body, name resolution fails at the reference.
    ::ankyra::klipper_output!(DebugPrint, value: u32 = 42);
}

fn main() {
    not_a_handler();
}
