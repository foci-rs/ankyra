use ankyra::SendOutput;
use ankyra_macros::klipper_output;

#[klipper_output]
pub struct Tick {
    pub count: u32,
}

struct MockSender {
    last: Option<u32>,
}

impl SendOutput<Tick> for MockSender {
    fn send(&mut self, payload: Tick) {
        self.last = Some(payload.count);
    }
}

/// Emitted from a plain function (no `#[klipper_command]` wrapper). The
/// sender is supplied explicitly via `klipper_output_from!`.
fn emit_tick(sender: &mut MockSender, count: u32) {
    ::ankyra::klipper_output_from!(sender, Tick, count: u32 = count);
}

fn main() {
    let mut sender = MockSender { last: None };
    emit_tick(&mut sender, 7);
    assert_eq!(sender.last, Some(7));
}
