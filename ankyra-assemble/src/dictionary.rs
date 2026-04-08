//! Build the Klipper data dictionary for the firmware.
//!
//! The dictionary is a JSON document the host retrieves via the `identify`
//! command. Klipper uses it to map protocol ids back to named commands,
//! replies, outputs, constants, enumerations, and build metadata. The full
//! shape is documented in the Klipper tree at `docs/Protocol.md`; we emit
//! a strict subset:
//!
//! ```json
//! {
//!   "version": "ankyra-v0.1",
//!   "build_versions": "<rustc + ankyra version>",
//!   "commands":  { "identify offset=%u count=%u": 1, "<fmt>": <id>, ... },
//!   "responses": { "identify_response offset=%u data=%.*s": 0,
//!                  "shutdown clock=%u static_string_id=%hu": <id>, ... },
//!   "output":    { "<fmt>": <id>, ... },
//!   "enumerations": { ... },
//!   "config": {},
//!   "static_strings": { "<id>": "<text>", ... }
//! }
//! ```
//!
//! # v0.1 scope limitation
//!
//! The unexpanded carrier-call fold used by `ankyra_config!` does not give
//! us the per-item `message_format` string at proc-macro time (the carrier
//! macro has not expanded yet — see `input::parse_carrier_call`). Task 12
//! therefore falls back to using the item's protocol name as a placeholder
//! format string for user-declared commands, replies, and outputs. The
//! three synthesized items (`identify`, `identify_response`, `shutdown`)
//! carry their Klipper-accurate formats verbatim because we own them. Task
//! 13 (the cross-crate clock example) will wire up a provider-metadata
//! carrier so authoritative formats flow through.
//!
//! Compression is also deferred: the firmware emits the dictionary
//! uncompressed, which simplifies the identify response handler. A future
//! revision may zlib-compress the bytes and streak them across multiple
//! `identify_response` frames.

use std::collections::BTreeMap;

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use serde_json::{Value, json};

use crate::identify::{
    IDENTIFY_CMD_NAME, IDENTIFY_RESPONSE_REPLY_NAME, SHUTDOWN_REPLY_NAME, identify_cmd_format,
    identify_response_reply_format, shutdown_reply_format,
};
use crate::sort::{AssembledItem, Assembly};

/// Build the dictionary bytes at proc-macro expansion time and emit a
/// `pub const DICT_BYTES: &[u8] = b"…";` item.
pub(crate) fn emit(assembly: &Assembly) -> TokenStream2 {
    let json_text = build_dictionary_json(assembly);
    // The firmware reads these bytes out of ROM via the `identify` response
    // handler; a byte-string literal plus a typed const keeps the data in
    // `.rodata` with a known length.
    let bytes = syn::LitByteStr::new(json_text.as_bytes(), proc_macro2::Span::call_site());
    let len = json_text.len();
    quote! {
        /// Uncompressed Klipper data dictionary JSON for this firmware.
        pub const DICT_BYTES: &[u8; #len] = #bytes;
    }
}

/// Assemble the full JSON document as a `String`. Separated from [`emit`]
/// so unit tests can parse and inspect the result without rendering tokens.
fn build_dictionary_json(assembly: &Assembly) -> String {
    let mut commands = BTreeMap::<String, Value>::new();
    let mut responses = BTreeMap::<String, Value>::new();
    let mut output = BTreeMap::<String, Value>::new();

    for item in assembly.items() {
        let (id, fmt) = (item.id, item_format(item));
        let bucket = match item.kind {
            "command" => &mut commands,
            "reply" => &mut responses,
            "output" => &mut output,
            other => panic!("unexpected item kind {other}"),
        };
        bucket.insert(fmt, Value::Number(id.into()));
    }

    let mut static_strings_obj = serde_json::Map::new();
    for (s, id) in assembly.static_strings() {
        static_strings_obj.insert(id.to_string(), Value::String(s.clone()));
    }

    let doc = json!({
        "version": "ankyra-v0.1",
        "build_versions": format!(
            "ankyra-{}",
            env!("CARGO_PKG_VERSION"),
        ),
        "app": "ankyra",
        "license": "MIT OR Apache-2.0",
        "commands": to_sorted_map(commands),
        "responses": to_sorted_map(responses),
        "output": to_sorted_map(output),
        "enumerations": {},
        "config": {},
        "static_strings": Value::Object(static_strings_obj),
    });
    // `to_string` is unstable-order free (serde_json preserves insertion
    // order for Maps), and we already sorted the per-bucket BTreeMaps above.
    serde_json::to_string(&doc).expect("dictionary JSON must serialize")
}

/// Canonical format string for an assembled item.
///
/// The three synthesized reserved items carry their Klipper-accurate
/// formats; everything else falls back to the protocol name alone (a
/// placeholder — see module docs).
fn item_format(item: &AssembledItem) -> String {
    match (item.kind, item.name) {
        ("command", IDENTIFY_CMD_NAME) => identify_cmd_format().to_string(),
        ("reply", IDENTIFY_RESPONSE_REPLY_NAME) => identify_response_reply_format().to_string(),
        ("reply", SHUTDOWN_REPLY_NAME) => shutdown_reply_format().to_string(),
        _ => {
            if let Some(fmt) = &item.message_format {
                fmt.clone()
            } else {
                // Placeholder: just the protocol name. See module docs for
                // why this is acceptable at v0.1 and how Task 13 will fix
                // it.
                item.name.to_string()
            }
        }
    }
}

/// Convert a `BTreeMap` (deterministic key order) into a `serde_json::Map`
/// with the same ordering. Needed because `json!({…})` macros cannot take
/// a runtime-built map directly.
fn to_sorted_map(map: BTreeMap<String, Value>) -> Value {
    let mut out = serde_json::Map::new();
    for (k, v) in map {
        out.insert(k, v);
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sort::{ItemInput, assemble};

    fn dict_of(items: Vec<ItemInput>, strings: Vec<String>) -> serde_json::Value {
        let a = assemble(items, strings).unwrap();
        let s = build_dictionary_json(&a);
        serde_json::from_str(&s).expect("dictionary JSON must parse")
    }

    #[test]
    fn includes_synthesized_identify_and_shutdown() {
        let doc = dict_of(vec![], vec![]);
        let cmds = doc["commands"].as_object().expect("commands object");
        let resps = doc["responses"].as_object().expect("responses object");
        assert!(cmds.contains_key("identify offset=%u count=%u"));
        assert!(resps.contains_key("identify_response offset=%u data=%.*s"));
        assert!(resps.contains_key("shutdown clock=%u static_string_id=%hu"));
        assert_eq!(
            resps["identify_response offset=%u data=%.*s"]
                .as_u64()
                .unwrap(),
            0
        );
        assert_eq!(cmds["identify offset=%u count=%u"].as_u64().unwrap(), 1);
    }

    #[test]
    fn user_command_lands_in_commands_with_placeholder_format() {
        let doc = dict_of(vec![ItemInput::command("ping")], vec![]);
        let cmds = doc["commands"].as_object().unwrap();
        // Placeholder format = protocol name alone.
        assert!(cmds.contains_key("ping"), "ping missing: {cmds:?}");
    }

    #[test]
    fn static_strings_are_recorded_by_id() {
        let doc = dict_of(vec![], vec!["alpha".into(), "beta".into()]);
        let ss = doc["static_strings"].as_object().unwrap();
        // `assemble` sorts lexicographically so alpha=2, beta=3.
        assert_eq!(ss["2"], json!("alpha"));
        assert_eq!(ss["3"], json!("beta"));
    }

    #[test]
    fn version_and_app_fields_present() {
        let doc = dict_of(vec![], vec![]);
        assert_eq!(doc["version"], json!("ankyra-v0.1"));
        assert_eq!(doc["app"], json!("ankyra"));
    }
}
