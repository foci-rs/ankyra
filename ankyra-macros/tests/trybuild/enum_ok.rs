use ankyra::descriptor::DefinitionKind;
use ankyra::encoding::ReadError;
use ankyra_macros::klipper_enumeration;

klipper_enumeration! {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum MotorKind(name = "motor_kind", rename_all = "snake_case") {
        BldcMotor,
        Stepper,
        #[klipper_enumeration(rename = "custom-name")]
        Special,
    }
}

fn main() {
    // Enum is emitted with the three variants.
    let a = MotorKind::BldcMotor;
    let b = MotorKind::Stepper;
    let c = MotorKind::Special;

    // `From<MotorKind> for u8` assigns 0, 1, 2 in declaration order.
    assert_eq!(u8::from(a), 0u8);
    assert_eq!(u8::from(b), 1u8);
    assert_eq!(u8::from(c), 2u8);

    // `TryFrom<u8>` round-trips.
    assert_eq!(MotorKind::try_from(0u8).unwrap(), MotorKind::BldcMotor);
    assert_eq!(MotorKind::try_from(1u8).unwrap(), MotorKind::Stepper);
    assert_eq!(MotorKind::try_from(2u8).unwrap(), MotorKind::Special);

    // Out-of-range values yield ReadError.
    let err: Result<MotorKind, ReadError> = MotorKind::try_from(3u8);
    assert!(err.is_err());

    // Descriptor fn exists and encodes the variant mapping with rename_all
    // applied and the per-variant rename honoured.
    let desc = __ankyra_descriptor_MotorKind();
    assert_eq!(desc.kind(), DefinitionKind::Enumeration);
    assert_eq!(desc.exported_name(), "motor_kind");
    assert_eq!(desc.value(), "bldc_motor=0,stepper=1,custom-name=2");
}
