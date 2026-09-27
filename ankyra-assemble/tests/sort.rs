//! `ankyra-assemble` is a proc-macro crate, so it cannot re-export its own
//! `sort` module to downstream consumers. Proc-macro crates are permitted
//! to export only items tagged `#[proc_macro]` / `#[proc_macro_derive]` /
//! `#[proc_macro_attribute]`; everything else — including ordinary
//! `pub mod` declarations — is rejected by rustc. The module cannot double
//! as a public library surface.

#[path = "../src/identify.rs"]
#[allow(dead_code)]
mod identify;

#[path = "../src/shared.rs"]
mod shared;

#[path = "../src/sort.rs"]
#[allow(dead_code)]
mod sort;

use sort::{AssemblyError, ItemInput, assemble};

fn cmd(n: &'static str) -> ItemInput {
    ItemInput::command(n)
}

fn rep(n: &'static str) -> ItemInput {
    ItemInput::reply(n)
}

fn no_strings() -> Vec<String> {
    vec![]
}

#[test]
fn identify_and_identify_response_are_synthesized() {
    let a = assemble(vec![], no_strings()).unwrap();
    let names: Vec<_> = a.items().iter().map(|i| (i.kind, i.name)).collect();
    assert!(names.contains(&("reply", "identify_response")));
    assert!(names.contains(&("command", "identify")));
    let ids: std::collections::HashMap<_, _> = a.items().iter().map(|i| (i.name, i.id)).collect();
    assert_eq!(ids["identify_response"], 0);
    assert_eq!(ids["identify"], 1);
}

#[test]
fn shutdown_reply_is_synthesized() {
    let a = assemble(vec![], no_strings()).unwrap();
    let ids: std::collections::HashMap<_, _> = a.items().iter().map(|i| (i.name, i.id)).collect();
    assert!(ids.contains_key("shutdown"));
    assert!(ids["shutdown"] > 1);
}

#[test]
fn canonical_sort_skips_reserved_ids() {
    let a = assemble(vec![cmd("a"), rep("b")], no_strings()).unwrap();
    let ids: std::collections::HashMap<_, _> = a.items().iter().map(|i| (i.name, i.id)).collect();
    assert_ne!(ids["a"], ids["b"]);
    assert!(ids["a"] > 1 && ids["b"] > 1);
}

#[test]
fn rejects_cross_kind_name_collision() {
    let err = assemble(vec![cmd("ping"), rep("ping")], no_strings()).unwrap_err();
    assert!(matches!(err, AssemblyError::DuplicateProtocolName { name } if name == "ping"));
}

#[test]
fn rejects_same_kind_collision() {
    let err = assemble(vec![cmd("x"), cmd("x")], no_strings()).unwrap_err();
    assert!(matches!(err, AssemblyError::DuplicateProtocolName { name } if name == "x"));
}

#[test]
fn static_strings_are_assigned_sorted_ids_starting_at_2() {
    let a = assemble(vec![], vec!["zeta".into(), "alpha".into()]).unwrap();
    let ids: std::collections::HashMap<_, _> = a
        .static_strings()
        .iter()
        .map(|(s, id)| (s.as_str(), *id))
        .collect();
    assert_eq!(ids["alpha"], 2);
    assert_eq!(ids["zeta"], 3);
}
