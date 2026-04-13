use ankyra::ScratchOutput;
use ankyra::encoding::Writable;
use ankyra::reply::OutputPayload;
use ankyra_macros::klipper_output;

#[klipper_output]
pub struct DebugPrint {
    pub value: u32,
    pub label: i16,
}

fn _assert_output_payload<T: OutputPayload>() {}

fn main() {
    // Marker trait is implemented.
    _assert_output_payload::<DebugPrint>();

    // Writable impl compiles and runs.
    let d = DebugPrint {
        value: 1,
        label: -2,
    };
    let mut out = ScratchOutput::<16>::new();
    <DebugPrint as Writable>::write(&d, &mut out);
    let _ = out.result();

    // Descriptor fn exists at module scope and returns an OutputDescriptor.
    // `DebugPrint` auto-converts to the `debug_print` wire name.
    let desc = __ankyra_descriptor_DebugPrint();
    assert_eq!(desc.protocol_name(), "debug_print");
    assert_eq!(desc.message_format(), "debug_print value=%u label=%hi");
}
