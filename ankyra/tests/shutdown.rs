use ankyra::encoding::{Readable, Writable};
use ankyra::{ScratchOutput, Shutdown, SliceInputBuffer};

#[test]
fn shutdown_writes_two_fields_in_order() {
    let s = Shutdown {
        clock: 0x1234_5678,
        static_string_id: 7,
    };
    let mut out = ScratchOutput::<64>::new();
    s.write(&mut out);
    let bytes = out.result();

    let mut input = SliceInputBuffer::new(bytes);
    let clock = <u32 as Readable>::read(&mut input).unwrap();
    let ssid = <u16 as Readable>::read(&mut input).unwrap();
    assert_eq!(clock, 0x1234_5678);
    assert_eq!(ssid, 7);
}
