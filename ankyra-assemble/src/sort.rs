//! Canonical sort, dedup, ID assignment, and identify/shutdown synthesis.
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
//! `Command < Reply < Output`. Names compare by raw byte order. The relative order of commands, replies,
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

use std::collections::{HashMap, HashSet};

use proc_macro2::TokenStream as TokenStream2;

use crate::identify::{
    IDENTIFY_CMD_ID, IDENTIFY_CMD_NAME, IDENTIFY_RESPONSE_REPLY_ID, IDENTIFY_RESPONSE_REPLY_NAME,
    SHUTDOWN_REPLY_NAME,
};
use ankyra_codegen::fnv1a_64;

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
/// The sort stage only uses `kind` and `name`; the remaining fields pass
/// through to the dictionary, dispatch, and sender emitters. Constants and
/// enumerations are carried through as their own kind variants in the
/// carrier tuple but are not sorted alongside commands/replies/outputs —
/// see [`crate::input::parse`] for how the parser routes them.
#[derive(Debug, Clone)]
pub struct ItemInput {
    pub kind: ItemKind,
    pub name: String,
    /// Number of lifetime parameters the struct carries. Populated from
    /// the `lt<N>_` infix in the carrier ident for `#[klipper_reply]` /
    /// `#[klipper_output]` structs; always `0` for command and definition
    /// kinds. Consumed by `senders::emit` to synthesise
    /// `impl<'a0, ..> SendReply<Struct<'a0, ..>> for Sender` headers.
    pub lifetime_count: usize,
    /// Klipper-style message format. Only inline-tuple fixtures populate
    /// it; the dictionary builder falls back to it when `sibling_scope` is
    /// `None`.
    pub message_format: Option<String>,
    /// Path to the `pub const fn __ankyra_descriptor_<N>()` emitted by
    /// `#[klipper_reply]` / `#[klipper_output]` / `#[klipper_constant]` /
    /// `klipper_enumeration!`. Read by the sender emitter.
    pub descriptor_path: Option<TokenStream2>,
    /// Path to the `__ankyra_dispatch_<name>` dispatch wrapper emitted by
    /// `#[klipper_command]`. Only populated for commands. Read by the
    /// dispatch emitter.
    pub dispatch_path: Option<TokenStream2>,
    /// Module scope where the item's sibling `__ANKYRA_*` consts live:
    /// `Some($crate)` for crate-root carrier-backed entries,
    /// `Some($crate::submod)` for submodule entries. `None` only for
    /// inline-tuple test fixtures and the three synthesized reserved items
    /// (`identify`, `identify_response`, `shutdown`).
    pub sibling_scope: Option<TokenStream2>,
}

impl ItemInput {
    /// Construct a command-kind item with only the name populated.
    pub fn command(name: impl Into<String>) -> Self {
        Self {
            kind: ItemKind::Command,
            name: name.into(),
            lifetime_count: 0,
            message_format: None,
            descriptor_path: None,
            dispatch_path: None,
            sibling_scope: None,
        }
    }

    /// Construct a reply-kind item with only the name populated.
    pub fn reply(name: impl Into<String>) -> Self {
        Self {
            kind: ItemKind::Reply,
            name: name.into(),
            lifetime_count: 0,
            message_format: None,
            descriptor_path: None,
            dispatch_path: None,
            sibling_scope: None,
        }
    }
}

/// An item after sort, dedup, and ID assignment.
#[derive(Debug, Clone)]
pub struct AssembledItem {
    pub kind: &'static str,
    /// See [`ItemInput::lifetime_count`].
    pub lifetime_count: usize,
    pub name: &'static str,
    pub id: u16,
    pub message_format: Option<String>,
    pub descriptor_path: Option<TokenStream2>,
    pub dispatch_path: Option<TokenStream2>,
    /// See [`ItemInput::sibling_scope`].
    pub sibling_scope: Option<TokenStream2>,
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
    /// Two commands or replies supplied the same protocol name. Outputs are
    /// keyed by format string instead and checked in the generated code.
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

    let mut working: Vec<ItemInput> = inputs;
    working.push(ItemInput::command(IDENTIFY_CMD_NAME));
    working.push(ItemInput::reply(IDENTIFY_RESPONSE_REPLY_NAME));
    working.push(ItemInput::reply(SHUTDOWN_REPLY_NAME));

    let mut seen: HashSet<String> = HashSet::with_capacity(working.len());
    for item in working.iter().filter(|i| i.kind != ItemKind::Output) {
        let wire_name = ankyra_codegen::item_wire_name(&item.name);
        if !seen.insert(wire_name.clone()) {
            return Err(AssemblyError::DuplicateProtocolName { name: wire_name });
        }
    }

    working.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.cmp(&b.name)));

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
        let leaked: &'static str = Box::leak(item.name.into_boxed_str());
        assembled.push(AssembledItem {
            kind: item.kind.tag(),
            name: leaked,
            id,
            lifetime_count: item.lifetime_count,
            message_format: item.message_format,
            descriptor_path: item.descriptor_path,
            dispatch_path: item.dispatch_path,
            sibling_scope: item.sibling_scope,
        });
    }

    let mut deduped: Vec<String> = Vec::with_capacity(static_strings.len());
    let mut seen_strings: HashSet<String> = HashSet::with_capacity(static_strings.len());
    for s in static_strings {
        if seen_strings.insert(s.clone()) {
            deduped.push(s);
        }
    }
    deduped.sort();

    let mut hash_index: HashMap<u64, String> = HashMap::with_capacity(deduped.len());
    for s in &deduped {
        let hash = fnv1a_64(s.as_bytes());
        if let Some(prev) = hash_index.insert(hash, s.clone()) {
            return Err(AssemblyError::StaticStringHashCollision {
                a: prev,
                b: s.clone(),
            });
        }
    }

    // Klipper reserves static-string ids 0 and 1.
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
        assert_eq!(by_name["shutdown"], ("reply", 2));
    }

    #[test]
    fn command_id_space_starts_at_two() {
        let a = assemble(vec![ItemInput::command("a")], vec![]).unwrap();
        let by_name: HashMap<_, _> = a.items().iter().map(|i| (i.name, i.id)).collect();
        assert_eq!(by_name["a"], 2);
        assert_eq!(by_name["shutdown"], 3);
    }
}
