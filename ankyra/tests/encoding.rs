use ankyra::ScratchOutput;
use ankyra::encoding::{ReadError, Readable, Writable};

#[test]
fn roundtrip_u32() {
    let mut out = ScratchOutput::<64>::new();
    <u32 as Writable>::write(&0x1234_5678, &mut out);
    let bytes = out.result();
    let mut cursor: &[u8] = bytes;
    assert_eq!(<u32 as Readable>::read(&mut cursor).unwrap(), 0x1234_5678);
}

#[test]
fn roundtrip_i32_negative() {
    let mut out = ScratchOutput::<64>::new();
    <i32 as Writable>::write(&-42, &mut out);
    let bytes = out.result();
    let mut cursor: &[u8] = bytes;
    assert_eq!(<i32 as Readable>::read(&mut cursor).unwrap(), -42);
}

#[test]
fn roundtrip_u16_boundary() {
    for &value in &[0u16, 1, 127, 128, 0xFFFF] {
        let mut out = ScratchOutput::<64>::new();
        <u16 as Writable>::write(&value, &mut out);
        let bytes = out.result();
        let mut cursor: &[u8] = bytes;
        assert_eq!(<u16 as Readable>::read(&mut cursor).unwrap(), value);
    }
}

#[test]
fn roundtrip_bool() {
    for &value in &[true, false] {
        let mut out = ScratchOutput::<64>::new();
        <bool as Writable>::write(&value, &mut out);
        let bytes = out.result();
        let mut cursor: &[u8] = bytes;
        assert_eq!(<bool as Readable>::read(&mut cursor).unwrap(), value);
    }
}

#[test]
fn roundtrip_byte_slice() {
    for payload in [&[1u8, 2, 3, 4][..], &[][..]] {
        let mut out = ScratchOutput::<64>::new();
        <&[u8] as Writable>::write(&payload, &mut out);
        let bytes = out.result();

        let mut cursor: &[u8] = bytes;
        let decoded = <&[u8] as Readable>::read(&mut cursor).unwrap();
        assert_eq!(decoded, payload);
        assert!(cursor.is_empty());
    }
}

#[test]
fn roundtrip_str() {
    for payload in ["hello", ""] {
        let mut out = ScratchOutput::<64>::new();
        <&str as Writable>::write(&payload, &mut out);
        let bytes = out.result();

        let mut cursor: &[u8] = bytes;
        let decoded = <&str as Readable>::read(&mut cursor).unwrap();
        assert_eq!(decoded, payload);
        assert!(cursor.is_empty());
    }
}

#[test]
fn truncated_input_errors() {
    let mut cursor: &[u8] = &[0x80];
    assert!(matches!(
        <u32 as Readable>::read(&mut cursor),
        Err(ReadError)
    ));
}
