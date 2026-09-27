use ankyra::formats_distinct;

#[test]
fn distinct_formats_pass() {
    assert!(formats_distinct(&["tick n=%u", "tock n=%u", "tick"]));
}

#[test]
fn a_repeated_format_fails() {
    assert!(!formats_distinct(&["tick n=%u", "tock", "tick n=%u"]));
}

#[test]
fn a_shared_prefix_is_not_a_repeat() {
    assert!(formats_distinct(&["tick", "tick n=%u"]));
}
