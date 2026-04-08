use ankyra::descriptor::{ItemKind, MessageDescriptor};
use ankyra::provider::{ProviderRef, ProviderSpec};

struct T;
impl ProviderSpec for T {
    const MESSAGES: &'static [MessageDescriptor] = &[
        MessageDescriptor::command("z_last", "z_last"),
        MessageDescriptor::command("a_first", "a_first"),
    ];
    // Per spec: providers do not own static strings. Static strings are
    // firmware-local and registered only in `ankyra_config!`.
}

#[test]
fn message_ord_is_kind_then_name() {
    let mut items = [
        MessageDescriptor::command("z", "z"),
        MessageDescriptor::reply("clock", "clock clock=%u"),
        MessageDescriptor::command("a", "a"),
        MessageDescriptor::output("#debug", "#debug value=%u"),
    ];
    items.sort();
    let order: Vec<_> = items
        .iter()
        .map(|i| (i.kind(), i.protocol_name()))
        .collect();
    assert_eq!(
        order,
        [
            (ItemKind::Command, "a"),
            (ItemKind::Command, "z"),
            (ItemKind::Reply, "clock"),
            (ItemKind::Output, "#debug"),
        ]
    );
}

#[test]
fn provider_ref_accessors() {
    let p = ProviderRef::new::<T>();
    assert_eq!(p.messages().len(), 2);
    assert_eq!(p.replies().len(), 0);
    assert_eq!(p.outputs().len(), 0);
    assert_eq!(p.definitions().len(), 0);
}
