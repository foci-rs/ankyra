use ankyra::encoding::{ReadError, Readable, Writable};
use ankyra::{InputBuffer, ScratchOutput, SliceInputBuffer};

#[test]
fn roundtrip_u32() {
    let mut out = ScratchOutput::<64>::new();
    <u32 as Writable>::write(&0x1234_5678, &mut out);
    let bytes = out.result();
    let mut input = SliceInputBuffer::new(bytes);
    assert_eq!(<u32 as Readable>::read(&mut input).unwrap(), 0x1234_5678);
}

#[test]
fn roundtrip_i32_negative() {
    let mut out = ScratchOutput::<64>::new();
    <i32 as Writable>::write(&-42, &mut out);
    let bytes = out.result();
    let mut input = SliceInputBuffer::new(bytes);
    assert_eq!(<i32 as Readable>::read(&mut input).unwrap(), -42);
}

#[test]
fn roundtrip_u16_boundary() {
    for &value in &[0u16, 1, 127, 128, 0xFFFF] {
        let mut out = ScratchOutput::<64>::new();
        <u16 as Writable>::write(&value, &mut out);
        let bytes = out.result();
        let mut input = SliceInputBuffer::new(bytes);
        assert_eq!(<u16 as Readable>::read(&mut input).unwrap(), value);
    }
}

#[test]
fn roundtrip_bool() {
    for &value in &[true, false] {
        let mut out = ScratchOutput::<64>::new();
        <bool as Writable>::write(&value, &mut out);
        let bytes = out.result();
        let mut input = SliceInputBuffer::new(bytes);
        assert_eq!(<bool as Readable>::read(&mut input).unwrap(), value);
    }
}

#[test]
fn roundtrip_byte_slice() {
    for payload in [&[1u8, 2, 3, 4][..], &[][..]] {
        let mut out = ScratchOutput::<64>::new();
        <&[u8] as Writable>::write(&payload, &mut out);
        let bytes = out.result();

        // Decode: VLQ length prefix, then raw bytes pulled from the InputBuffer.
        let mut input = SliceInputBuffer::new(bytes);
        let len = <u32 as Readable>::read(&mut input).unwrap() as usize;
        assert_eq!(len, payload.len());
        let data = &input.data()[..len];
        assert_eq!(data, payload);
        input.pop(len);
        assert_eq!(input.available(), 0);
    }
}

#[test]
fn roundtrip_str() {
    for payload in ["hello", ""] {
        let mut out = ScratchOutput::<64>::new();
        <&str as Writable>::write(&payload, &mut out);
        let bytes = out.result();

        let mut input = SliceInputBuffer::new(bytes);
        let len = <u32 as Readable>::read(&mut input).unwrap() as usize;
        assert_eq!(len, payload.len());
        let data = &input.data()[..len];
        assert_eq!(core::str::from_utf8(data).unwrap(), payload);
        input.pop(len);
        assert_eq!(input.available(), 0);
    }
}

#[test]
fn truncated_input_errors() {
    let mut input = SliceInputBuffer::new(&[0x80]); // multi-byte VLQ with no continuation
    assert!(matches!(
        <u32 as Readable>::read(&mut input),
        Err(ReadError)
    ));
}
