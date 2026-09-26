#[path = "../src/identify.rs"]
#[allow(dead_code)]
mod identify;

#[path = "../src/shared.rs"]
mod shared;

#[path = "../src/sort.rs"]
#[allow(dead_code)]
mod sort;

#[path = "../src/senders.rs"]
#[allow(dead_code)]
mod senders;

#[test]
fn assembler_frame_budget_matches_ankyra_constant() {
    assert_eq!(
        senders::MESSAGE_PAYLOAD_MAX,
        ankyra::MESSAGE_PAYLOAD_MAX,
        "ankyra-assemble's diagnostic-only MESSAGE_PAYLOAD_MAX has drifted \
         from ankyra::MESSAGE_PAYLOAD_MAX; update the literal in senders.rs"
    );
}
