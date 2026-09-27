use ankyra::SendOutput;
use ankyra_macros::klipper_output;

#[klipper_output]
pub struct Tick {
    pub count: u32,
}

fn emit_tick<S: SendOutput<Tick>>(sender: &mut S) {
    ::ankyra::klipper_output_from!(sender, Tick, count: u8 = 7);
}

fn main() {}
