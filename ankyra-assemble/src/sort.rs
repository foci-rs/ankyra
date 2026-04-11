//! Canonical sort, dedup, ID assignment, and identify/shutdown synthesis.
//!
//! The assembler's sort stage is separated from the proc-macro entry point
//! for two reasons. First, the algorithm is pure data in/out and benefits
//! from being exercisable through ordinary `cargo test` on a host target
//! without expanding a proc-macro. Second, Task 12 will grow this module
//! with dictionary JSON emission, dispatch match arm rendering, and sender
//! impl synthesis that all hang off of `Assembly` — keeping the sort stage
//! in its own module lets those additions slot in without refactoring
//! `lib.rs`.
//!
//! # Reserved names and IDs
//!
//! Three names are owned by the assembler:
//!
//! * `identify` — a command, always id 1.
//! * `identify_response` — a reply, always id 0.
//! * `shutdown` — a reply, whose id is chosen by the canonical sort.
//!
//! User-supplied items that would shadow any of those names are rejected
//! with [`AssemblyError::ReservedNameShadowed`]. The three reserved items
//! are injected after the shadow check, so a user cannot bypass the check
//! by declaring one of them and relying on dedup.
//!
//! # Canonical sort
//!
//! Items are sorted by `(kind, name)` where `ItemKind` orders as
//! `Command < Reply < Output`. Names compare by raw byte order, which
//! matches the ASCII ordering Python's `sorted()` uses on the Klipper host
//! side of existing protocols. The relative order of commands, replies,
//! and outputs is stable within a kind.
//!
//! # ID assignment
//!
//! After sorting:
//!
//! * `identify_response` gets id 0.
//! * `identify` gets id 1.
//! * All remaining commands (in sort order) get ids 2, 3, 4, ...
//! * All remaining replies continue the counter after the last command.
//! * All outputs continue the counter after the last reply.
//!
//! Static strings are handled separately; see [`assemble`] for their
//! ID assignment rules.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use proc_macro2::TokenStream as TokenStream2;

use crate::identify::{
    IDENTIFY_CMD_ID, IDENTIFY_CMD_NAME, IDENTIFY_RESPONSE_REPLY_ID, IDENTIFY_RESPONSE_REPLY_NAME,
    SHUTDOWN_REPLY_NAME,
};
use crate::shared::fnv1a_64;

/// Kind of a protocol item. The discriminant order is significant — the
/// canonical sort in [`assemble`] orders items by `(kind, name)` and ID
/// assignment walks the kinds in `Command < Reply < Output` order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ItemKind {
    Command = 0,
    Reply = 1,
    Output = 2,
}

impl ItemKind {
    /// Stable `&'static str` tag used by downstream consumers and tests.
    /// Matches the bare ident emitted in carrier tuples
    /// (`command`/`reply`/`output`).
    pub fn tag(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Reply => "reply",
            Self::Output => "output",
        }
    }
}

/// One protocol item fed into [`assemble`].
///
/// For Task 10 we only use `kind` and `name`. Task 12 and D1 read
/// `message_format`, `descriptor_path`, `dispatch_path`, and `carrier_path`
/// to emit the data dictionary and dispatch match arms. Constants and
/// enumerations are carried through as their own kind variants in the
/// carrier tuple but are not sorted alongside commands/replies/outputs —
/// see [`parse_items`](crate::input::parse) for how the parser routes
/// them.
#[derive(Debug, Clone)]
pub struct ItemInput {
    pub kind: ItemKind,
    /// Protocol-facing name. Stored as a `String` so synthesized items
    /// (`identify`, `shutdown`, etc.) can own their names without leaking
    /// `'static` storage.
    pub name: String,
    /// Klipper-style message format. Task 10 does not use this; it is
    /// threaded through so Task 12 can read it from the carrier tuples
    /// without re-parsing the input.
    pub message_format: Option<String>,
    /// Path to the `pub const fn __ankyra_descriptor_<N>()` emitted by
    /// `#[klipper_reply]` / `#[klipper_output]` / `#[klipper_constant]` /
    /// `klipper_enumeration!`. Unused by Task 10.
    pub descriptor_path: Option<TokenStream2>,
    /// Path to the `__ankyra_dispatch_<name>` dispatch wrapper emitted by
    /// `#[klipper_command]`. Only populated for commands. Unused by
    /// Task 10.
    pub dispatch_path: Option<TokenStream2>,
    /// Path to the `__ankyra_item_<kind>_<name>!` carrier macro emitted
    /// by each `#[klipper_*]` / `klipper_enumeration!` invocation. D1
    /// uses this to invoke `<path>!(name)` / `<path>!(format)` /
    /// `<path>!(value)` inside `const_format::concatcp!` so the
    /// dictionary JSON picks up real user-supplied format strings and
    /// constant/enumeration values at const-eval time rather than
    /// placeholder text at proc-macro time.
    pub carrier_path: Option<TokenStream2>,
    /// Module prefix for submodule items: `Some($crate::submod)` when the
    /// provider registered this item at a path, `None` for crate-root
    /// items. Supplied by the wrapped-carrier form of the assembler
    /// input (see `ankyra-assemble/src/input.rs::parse_wrapped_carrier_call`
    /// added in Task C4). Consumed by `dictionary::push_format` (Task C5)
    /// to resolve `__ANKYRA_FORMAT_<kind>_<name>` at the item's defining
    /// scope.
    pub module_prefix: Option<TokenStream2>,
}

impl ItemInput {
    /// Construct a command-kind item with only the name populated.
    /// Convenient for the unit tests in `tests/sort.rs`.
    pub fn command(name: impl Into<String>) -> Self {
        Self {
            kind: ItemKind::Command,
            name: name.into(),
            message_format: None,
            descriptor_path: None,
            dispatch_path: None,
            carrier_path: None,
            module_prefix: None,
        }
    }

    /// Construct a reply-kind item with only the name populated.
    pub fn reply(name: impl Into<String>) -> Self {
        Self {
            kind: ItemKind::Reply,
            name: name.into(),
            message_format: None,
            descriptor_path: None,
            dispatch_path: None,
            carrier_path: None,
            module_prefix: None,
        }
    }

    /// Construct an output-kind item with only the name populated.
    pub fn output(name: impl Into<String>) -> Self {
        Self {
            kind: ItemKind::Output,
            name: name.into(),
            message_format: None,
            descriptor_path: None,
            dispatch_path: None,
            carrier_path: None,
            module_prefix: None,
        }
    }
}

/// An item after sort, dedup, and ID assignment.
///
/// `kind` is exposed as `&'static str` rather than the [`ItemKind`] enum so
/// downstream consumers (tests, Task 12's dictionary emitter) can match on
/// the same tag the carrier tuples use without depending on this crate's
/// enum shape.
#[derive(Debug, Clone)]
pub struct AssembledItem {
    pub kind: &'static str,
    /// Protocol-facing name. Leaked to `&'static str` so it can be threaded
    /// through `quote!` calls that expect `&'static str` literals without
    /// having to re-allocate. The leaks are bounded by the number of
    /// distinct protocol names a single `ankyra_config!` invocation
    /// produces, which is small and known at compile time.
    pub name: &'static str,
    pub id: u16,
    pub message_format: Option<String>,
    pub descriptor_path: Option<TokenStream2>,
    pub dispatch_path: Option<TokenStream2>,
    pub carrier_path: Option<TokenStream2>,
}

/// Output of the sort stage. Downstream code accesses the item list and
/// the static-string table through read-only accessors; the assembler
/// never mutates an `Assembly` after construction.
#[derive(Debug, Clone)]
pub struct Assembly {
    items: Vec<AssembledItem>,
    static_strings: Vec<(String, u16)>,
}

impl Assembly {
    pub fn items(&self) -> &[AssembledItem] {
        &self.items
    }

    pub fn static_strings(&self) -> &[(String, u16)] {
        &self.static_strings
    }
}

/// Errors reported by [`assemble`]. Every variant owns the offending
/// name(s) so the proc-macro entry point can render a span-free
/// diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssemblyError {
    /// Two items supplied the same protocol name, either within a kind or
    /// across kinds.
    DuplicateProtocolName { name: String },
    /// A user-supplied item tried to use one of the three reserved names
    /// (`identify`, `identify_response`, `shutdown`).
    ReservedNameShadowed { name: String },
    /// Two distinct static strings hashed to the same FNV-1a-64 value.
    /// Both strings are reported to help the user choose a new literal.
    StaticStringHashCollision { a: String, b: String },
}

impl std::fmt::Display for AssemblyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateProtocolName { name } => {
                write!(f, "duplicate protocol name `{name}`")
            }
            Self::ReservedNameShadowed { name } => {
                write!(
                    f,
                    "item name `{name}` is reserved by the ankyra assembler \
                     (one of: identify, identify_response, shutdown)"
                )
            }
            Self::StaticStringHashCollision { a, b } => {
                write!(
                    f,
                    "static strings `{a}` and `{b}` collide under FNV-1a-64; \
                     rename one of them"
                )
            }
        }
    }
}

impl std::error::Error for AssemblyError {}

/// Sort, dedup, assign IDs, synthesize reserved items, and compute the
/// static-string table.
///
/// See the module docs for the full algorithm description.
pub fn assemble(
    inputs: Vec<ItemInput>,
    static_strings: Vec<String>,
) -> Result<Assembly, AssemblyError> {
    // Reject user items that collide with the three reserved names before
    // appending the synthesized items — otherwise a user-supplied
    // `identify` would dedup silently against the injected one.
    for item in &inputs {
        if matches!(
            item.name.as_str(),
            IDENTIFY_CMD_NAME | IDENTIFY_RESPONSE_REPLY_NAME | SHUTDOWN_REPLY_NAME
        ) {
            return Err(AssemblyError::ReservedNameShadowed {
                name: item.name.clone(),
            });
        }
    }

    // Build the working list: user items + three synthesized reserved
    // items. Shadow check above guarantees we cannot introduce a duplicate
    // via the synthesized entries.
    let mut working: Vec<ItemInput> = inputs;
    working.push(ItemInput::command(IDENTIFY_CMD_NAME));
    working.push(ItemInput::reply(IDENTIFY_RESPONSE_REPLY_NAME));
    working.push(ItemInput::reply(SHUTDOWN_REPLY_NAME));

    // Global duplicate-name check (across kinds). Using a `HashMap` rather
    // than sorting-and-dedup because we want to report the duplicate name
    // directly, not diff a sorted list.
    let mut seen: HashMap<String, ItemKind> = HashMap::with_capacity(working.len());
    for item in &working {
        if let Some(_prev_kind) = seen.insert(item.name.clone(), item.kind) {
            return Err(AssemblyError::DuplicateProtocolName {
                name: item.name.clone(),
            });
        }
    }

    // Canonical sort: kind first (Command < Reply < Output), then name by
    // raw byte order.
    working.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.cmp(&b.name)));

    // ID assignment. We walk the sorted list and give the two reserved
    // items their fixed IDs, then hand out consecutive ids starting at 2
    // to everything else. Because commands come before replies come before
    // outputs in the sort, this naturally keeps commands first, replies
    // second, outputs last.
    let mut assembled: Vec<AssembledItem> = Vec::with_capacity(working.len());
    let mut next_id: u16 = 2;
    for item in working {
        let id = match (item.kind, item.name.as_str()) {
            (ItemKind::Command, name) if name == IDENTIFY_CMD_NAME => IDENTIFY_CMD_ID,
            (ItemKind::Reply, name) if name == IDENTIFY_RESPONSE_REPLY_NAME => {
                IDENTIFY_RESPONSE_REPLY_ID
            }
            _ => {
                let id = next_id;
                next_id = next_id
                    .checked_add(1)
                    .expect("protocol item count exceeds u16::MAX");
                id
            }
        };
        // Leak the name to `&'static str` so downstream consumers can
        // thread it through `quote!` without re-allocating. See
        // `AssembledItem::name` docs.
        let leaked: &'static str = Box::leak(item.name.into_boxed_str());
        assembled.push(AssembledItem {
            kind: item.kind.tag(),
            name: leaked,
            id,
            message_format: item.message_format,
            descriptor_path: item.descriptor_path,
            dispatch_path: item.dispatch_path,
            carrier_path: item.carrier_path,
        });
    }

    // Static strings: dedup preserving first occurrence, ASCII-sort, check
    // for hash collisions, then assign IDs starting at 2.
    let mut deduped: Vec<String> = Vec::with_capacity(static_strings.len());
    let mut seen_strings: HashSet<String> = HashSet::with_capacity(static_strings.len());
    for s in static_strings {
        if seen_strings.insert(s.clone()) {
            deduped.push(s);
        }
    }
    deduped.sort();

    // Hash-collision check. Klipper reserves ids 0 and 1 for internal use,
    // so static-string ids start at 2.
    let mut hash_index: HashMap<u64, String> = HashMap::with_capacity(deduped.len());
    for s in &deduped {
        let hash = fnv1a_64(s.as_bytes());
        if let Some(prev) = hash_index.insert(hash, s.clone()) {
            if &prev != s {
                return Err(AssemblyError::StaticStringHashCollision {
                    a: prev,
                    b: s.clone(),
                });
            }
        }
    }

    let static_strings: Vec<(String, u16)> = deduped
        .into_iter()
        .enumerate()
        .map(|(i, s)| {
            let id = u16::try_from(i + 2).expect("static-string count exceeds u16::MAX - 2");
            (s, id)
        })
        .collect();

    Ok(Assembly {
        items: assembled,
        static_strings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_inputs_synthesize_three_reserved_items() {
        let a = assemble(vec![], vec![]).unwrap();
        assert_eq!(a.items().len(), 3);
        let by_name: HashMap<_, _> = a.items().iter().map(|i| (i.name, (i.kind, i.id))).collect();
        assert_eq!(by_name["identify_response"], ("reply", 0));
        assert_eq!(by_name["identify"], ("command", 1));
        // shutdown gets the next id slot after identify (2).
        assert_eq!(by_name["shutdown"], ("reply", 2));
    }

    #[test]
    fn command_id_space_starts_at_two() {
        let a = assemble(vec![ItemInput::command("a")], vec![]).unwrap();
        let by_name: HashMap<_, _> = a.items().iter().map(|i| (i.name, i.id)).collect();
        // identify_response=0, identify=1, a=2, shutdown=3.
        assert_eq!(by_name["a"], 2);
        assert_eq!(by_name["shutdown"], 3);
    }
}

#[cfg(test)]
mod module_prefix_tests {
    use super::*;

    #[test]
    fn default_module_prefix_is_none() {
        let item = ItemInput::command("foo");
        assert!(item.module_prefix.is_none());
    }

    #[test]
    fn module_prefix_is_clone_and_debug() {
        // Compile-only check: the field must work with the rest of the
        // struct's derives. Also proves Clone + Debug + Send + Sync work.
        let mut item = ItemInput::command("foo");
        item.module_prefix = Some(quote::quote!($crate::sub));
        let cloned = item.clone();
        let _ = format!("{cloned:?}");
    }
}
