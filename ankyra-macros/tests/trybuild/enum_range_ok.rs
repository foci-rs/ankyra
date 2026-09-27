use ankyra::descriptor::DefinitionKind;
use ankyra_macros::klipper_enumeration;

klipper_enumeration! {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum Pin(name = "pin", rename_all = "snake_case") {
        Led,
        Range(gpio, 0, 4),
    }
}

fn main() {
    // Led is the first variant -> id 0.
    assert_eq!(u8::from(Pin::Led), 0u8);

    // Range expands to gpio0..gpio3 occupying ids 1..=4.
    assert_eq!(u8::from(Pin::gpio0), 1u8);
    assert_eq!(u8::from(Pin::gpio1), 2u8);
    assert_eq!(u8::from(Pin::gpio2), 3u8);
    assert_eq!(u8::from(Pin::gpio3), 4u8);

    assert_eq!(Pin::try_from(1u8).unwrap(), Pin::gpio0);
    assert_eq!(Pin::try_from(4u8).unwrap(), Pin::gpio3);
    assert!(Pin::try_from(5u8).is_err());

    let desc = __ankyra_descriptor_Pin();
    assert_eq!(desc.kind(), DefinitionKind::Enumeration);
    assert_eq!(desc.exported_name(), "pin");
    assert_eq!(
        desc.value(),
        r#"{"led":0,"gpio0":[1,4]}"#
    );
}
