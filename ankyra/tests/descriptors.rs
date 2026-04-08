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

#[test]
#[allow(clippy::nonminimal_bool)]
fn message_ord_tiebreaks_on_message_format_when_kind_and_name_match() {
    use core::cmp::Ordering;
    let a = MessageDescriptor::command("ping", "ping");
    let b = MessageDescriptor::command("ping", "ping seq=%u");
    assert_eq!(a.cmp(&b), Ordering::Less);
    assert_ne!(a, b);
    // Ord consistent with Eq: a.cmp(&b) == Equal iff a == b
    assert!(!(a == b) || a.cmp(&b) == Ordering::Equal);
    assert!(!(a.cmp(&b) == Ordering::Equal) || a == b);
}

#[test]
fn item_kind_ord_is_pinned() {
    use ankyra::descriptor::ItemKind;
    assert!(ItemKind::Command < ItemKind::Reply);
    assert!(ItemKind::Reply < ItemKind::Output);
}
