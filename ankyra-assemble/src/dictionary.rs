//! Build the Klipper data dictionary for the firmware.
//!
//! The dictionary is a JSON document the host retrieves via the `identify`
//! command. Klipper uses it to map protocol ids back to named commands,
//! replies, outputs, constants, enumerations, and build metadata. The full
//! shape is documented in the Klipper tree at `docs/Protocol.md`; we emit
//! the following subset (insertion order matches Klipper's `mcu.py`
//! reader expectations):
//!
//! ```json
//! {
//!   "commands":  { "identify offset=%u count=%u": 1,
//!                  "<fmt>": <id>, ... },
//!   "responses": { "identify_response offset=%u data=%.*s": 0,
//!                  "<name>": [<id>, "<fmt>"], ... },
//!   "output":    { "<name>": [<id>, "<fmt>"], ... },
//!   "config":    { "CLOCK_FREQ": 168000000, "MCU": "stm32f407", ... },
//!   "enumerations": { "motor_kind": {"bldc_motor": 0}, ... },
//!   "static_strings": { "<id>": "<text>", ... },
//!   "version":        "ankyra-v0.1",
//!   "build_versions": "ankyra-0.1.0",
//!   "app":            "ankyra",
//!   "license":        "MIT OR Apache-2.0"
//! }
//! ```
//!
//! # Compile-time assembly via `const_format::concatcp!`
//!
//! Proc-macros cannot read the `const` values they process — so at
//! `__ankyra_assemble!` expansion time we only see the carrier-macro
//! paths the `#[klipper_*]` attributes emitted, not their format strings.
//! To recover those strings without threading them through `ProviderSpec`
//! (an alternative we considered and rejected on ergonomics grounds),
//! the dictionary JSON is assembled at **const-eval time** via
//! `const_format::concatcp!`.
//!
//! Each user item emits a sibling `pub const __ANKYRA_FORMAT_<kind>_<name>: &str`
//! (and `__ANKYRA_VALUE_<kind>_<name>` for constants/enumerations) next
//! to its descriptor fn. The assembler reconstructs that path from the
//! carrier-macro prefix — swapping `__ankyra_item_` for `__ANKYRA_FORMAT_`
//! — and splices the resulting path into the `concatcp!` argument list.
//! At const-eval time, rustc substitutes each path with its stringified
//! value and `concatcp!` stitches the full JSON document.
//!
//! Why `pub const` paths rather than invoking the carrier macro's
//! `(format)` arm directly? rust-lang/rust#52234 rejects absolute paths
//! to same-crate `#[macro_export]` macros, which is the shape that
//! results whenever `ankyra_config!` and the `#[klipper_*]`-decorated
//! items live in the same crate. `pub const` items are not subject to
//! that restriction.
//!
//! The generated code looks like:
//!
//! ```ignore
//! const __ANKYRA_DICT_STR: &str = ::ankyra::const_format::concatcp!(
//!     "{\"commands\":{\"identify offset=%u count=%u\":", 1u16,
//!     ",\"", crate::__ANKYRA_FORMAT_command_ping, "\":", 2u16,
//!     // ...
//! );
//! pub const DICT_BYTES: &[u8] = __ANKYRA_DICT_STR.as_bytes();
//! ```
//!
//! # Why `concatcp!` over a hand-rolled runtime builder
//!
//! Ankyra ships `#![no_std]` MCU firmware; allocating the dictionary at
//! boot would trade ROM savings for heap pressure and (potentially) boot-
//! time latency. `concatcp!` gives us a fully static `&[u8]` that lives
//! in `.rodata`, so the identify handler can slice it without touching
//! the stack.
//!
//! # Inline-tuple fallback
//!
//! The assembler supports two carrier shapes: the unexpanded-macro-call
//! shape (the one proc-macros emit in practice) and a parenthesized
//! inline tuple used by the integration tests and synthetic fixtures.
//! Inline-tuple items carry their `message_format` directly — they do
//! not have a carrier path — so the dictionary builder inlines the
//! format string as a literal rather than referencing a `pub const`.

use proc_macro2::{Literal, TokenStream as TokenStream2};
use quote::quote;

use crate::identify::{
    IDENTIFY_CMD_NAME, IDENTIFY_RESPONSE_REPLY_NAME, SHUTDOWN_REPLY_NAME, identify_cmd_format,
    identify_response_reply_format, shutdown_reply_format,
};
use crate::input::{DefinitionInput, DefinitionKind};
use crate::sort::{AssembledItem, Assembly};

/// Emit the `__ANKYRA_DICT` string constant and the `DICT_BYTES` slice.
///
/// The caller splices this output into the `_ankyra_config` module tree.
pub(crate) fn emit(assembly: &Assembly, definitions: &[DefinitionInput]) -> TokenStream2 {
    let fragments = build_concatcp_args(assembly, definitions);
    quote! {
        /// Uncompressed Klipper data dictionary JSON for this firmware.
        ///
        /// Assembled at const-eval time via `const_format::concatcp!`
        /// from per-item carrier-macro arms, so user-supplied format
        /// strings and constant/enumeration values are stitched in as
        /// literal text — no runtime string building.
        #[doc(hidden)]
        pub const __ANKYRA_DICT_STR: &str = ::ankyra::const_format::concatcp!(
            #(#fragments),*
        );

        /// Uncompressed dictionary bytes. The identify handler
        /// zlib-compresses a slice of this buffer into a stack scratch
        /// buffer before streaming it back to the host — Klipper's
        /// host runs `zlib.decompress()` on the frames before parsing
        /// JSON.
        pub const DICT_BYTES: &[u8] = __ANKYRA_DICT_STR.as_bytes();
    }
}

/// Build the comma-separated expression list that sits inside the
/// `concatcp!(...)` invocation.
///
/// Each element is a `TokenStream2` that evaluates to a `&'static str`,
/// an integer primitive, or a `char`/`bool` (the shapes `concatcp!`
/// accepts). We interleave literal JSON fragments (`r#"...,""#`) with
/// carrier-macro-arm invocations (`<path>!(name)`) and numeric IDs.
fn build_concatcp_args(assembly: &Assembly, definitions: &[DefinitionInput]) -> Vec<TokenStream2> {
    let mut args: Vec<TokenStream2> = Vec::new();

    // Opening brace + commands section.
    push_literal(&mut args, "{\"commands\":{");
    emit_command_entries(&mut args, assembly);
    push_literal(&mut args, "},\"responses\":{");
    emit_reply_entries(&mut args, assembly);
    push_literal(&mut args, "},\"output\":{");
    emit_output_entries(&mut args, assembly);
    push_literal(&mut args, "},\"config\":{");
    emit_constant_entries(&mut args, definitions);
    push_literal(&mut args, "},\"enumerations\":{");
    emit_enumeration_entries(&mut args, definitions);
    push_literal(&mut args, "},\"static_strings\":{");
    emit_static_string_entries(&mut args, assembly);
    // Trailer: fixed metadata fields.
    push_literal(&mut args, "},");
    push_literal(
        &mut args,
        concat!(
            "\"version\":\"ankyra-v0.1\",",
            "\"build_versions\":\"ankyra-",
            env!("CARGO_PKG_VERSION"),
            "\",",
            "\"app\":\"ankyra\",",
            "\"license\":\"MIT OR Apache-2.0\"",
            "}",
        ),
    );
    args
}

/// Push one literal fragment as a `concatcp!` argument.
fn push_literal(args: &mut Vec<TokenStream2>, lit: &str) {
    let lit_tok = Literal::string(lit);
    args.push(quote!(#lit_tok));
}

/// Push an expression that evaluates to a `u16` dictionary id.
fn push_id(args: &mut Vec<TokenStream2>, id: u16) {
    let id_lit = Literal::u16_suffixed(id);
    args.push(quote!(#id_lit));
}

/// Push an expression that evaluates to a `&'static str` containing the
/// item's message format. For carrier-backed items we reference the
/// sibling `pub const __ANKYRA_FORMAT_<kind>_<name>` emitted by each
/// `#[klipper_*]` attribute; for inline-tuple items (no carrier path)
/// we embed the format as a string literal.
///
/// Using a `pub const` path (rather than invoking the carrier macro
/// directly) sidesteps rust-lang/rust#52234: same-crate
/// `#[macro_export]` macros cannot be referred to by absolute paths,
/// but `pub const` items can.
///
/// # Path resolution strategy
///
/// For wrapped-form items (`{ prefix: (…), carrier!() }`), the carrier
/// macro is `#[macro_export]`-hoisted to the crate root, so the carrier
/// path is `$crate::__ankyra_item_<kind>_<name>`. Applying the trailing-
/// segment rewrite to that yields `$crate::__ANKYRA_FORMAT_<kind>_<name>`,
/// which is wrong for submodule items because the FORMAT const lives at
/// `crate::submod::__ANKYRA_FORMAT_<kind>_<name>`.
///
/// When `item.module_prefix` is `Some(prefix)`, the FORMAT const is at
/// `<prefix>::__ANKYRA_FORMAT_<kind>_<name>` — we construct that path
/// directly. When `module_prefix` is `None` (crate-root items, synthesized
/// reserved items), we fall back to the carrier-path trailing-segment rewrite
/// which remains correct.
/// Push an expression that evaluates to the protocol-facing name string
/// for a command/reply/output item.
///
/// Mirrors [`push_format`]: when `item.module_prefix` is `Some(prefix)`,
/// the sibling `__ANKYRA_NAME_<kind>_<name>` const lives at `<prefix>`.
/// Otherwise we rewrite the carrier path's trailing segment. As a last
/// resort (inline-tuple test fixtures) we fall back to the parsed
/// `item.name` as a string literal.
///
/// Indirecting through the NAME const matters once `#[klipper_reply]` /
/// `#[klipper_output]` auto-convert `PascalCase` struct idents to
/// `snake_case` on the wire: the macro-emitted const always carries the
/// derived wire name, so the assembler picks up the post-conversion form
/// without duplicating the conversion rule here.
fn push_item_name(args: &mut Vec<TokenStream2>, item: &AssembledItem) {
    if let Some(prefix) = &item.module_prefix {
        let const_ident_str = format!("__ANKYRA_NAME_{}_{}", item.kind, item.name);
        let const_ident: syn::Ident = syn::parse_str(&const_ident_str)
            .expect("__ANKYRA_NAME_<kind>_<name> is always a valid ident");
        args.push(quote!(#prefix::#const_ident));
        return;
    }
    if let Some(path) = &item.carrier_path {
        if let Some(const_path) = sibling_const_path(path, "__ANKYRA_NAME_") {
            args.push(const_path);
            return;
        }
    }
    let name_lit = Literal::string(item.name);
    args.push(quote!(#name_lit));
}

fn push_format(args: &mut Vec<TokenStream2>, item: &AssembledItem) {
    // Submodule items: FORMAT const lives at the module where
    // #[klipper_command] (etc.) emitted it, not at the crate root.
    if let Some(prefix) = &item.module_prefix {
        let const_ident_str = format!("__ANKYRA_FORMAT_{}_{}", item.kind, item.name);
        let const_ident: syn::Ident = syn::parse_str(&const_ident_str)
            .expect("__ANKYRA_FORMAT_<kind>_<name> is always a valid ident");
        args.push(quote!(#prefix::#const_ident));
        return;
    }
    // Bare-carrier fallback (crate-root items, synthesized items): derive
    // the FORMAT path from the carrier macro path's trailing segment.
    if let Some(path) = &item.carrier_path {
        if let Some(const_path) = sibling_const_path(path, "__ANKYRA_FORMAT_") {
            args.push(const_path);
            return;
        }
    }
    // Last-resort fallback: embed the format as a string literal.
    // Used by inline-tuple test fixtures that carry no carrier path.
    let fmt = item
        .message_format
        .as_deref()
        .unwrap_or(item.name)
        .to_string();
    let fmt_lit = Literal::string(&fmt);
    args.push(quote!(#fmt_lit));
}

/// Strip an `<kind>_lt<N>_` prefix from a carrier-ident suffix and return
/// the normalised `<kind>_<name>` form. When no `lt<N>_` segment is
/// present (the common case) the input is returned as a borrowed slice.
fn strip_lt_infix(suffix: &str) -> String {
    // suffix looks like "reply_lt1_FooReply" or "reply_FooReply". Split at
    // the first underscore so we can inspect the remainder without
    // allocating.
    let Some(underscore_idx) = suffix.find('_') else {
        return suffix.to_string();
    };
    let (kind, rest_with_underscore) = suffix.split_at(underscore_idx);
    let rest = &rest_with_underscore[1..];
    // Reject non-reply/non-output kinds immediately — `lt<N>_` is only
    // meaningful there.
    if kind != "reply" && kind != "output" {
        return suffix.to_string();
    }
    if let Some(after_lt) = rest.strip_prefix("lt")
        && let Some(inner_underscore) = after_lt.find('_')
    {
        let (count_str, name_with_underscore) = after_lt.split_at(inner_underscore);
        if count_str.parse::<usize>().is_ok() {
            return format!("{}_{}", kind, &name_with_underscore[1..]);
        }
    }
    suffix.to_string()
}

/// Rewrite `<prefix>::__ankyra_item_<kind>_<name>` into
/// `<prefix>::<const_prefix><kind>_<name>` — e.g.
/// `::clock_lib::__ankyra_item_command_get_clock` →
/// `::clock_lib::__ANKYRA_FORMAT_command_get_clock`.
///
/// Returns `None` if the path cannot be parsed or does not end with the
/// expected carrier-ident prefix.
fn sibling_const_path(carrier_path: &TokenStream2, const_prefix: &str) -> Option<TokenStream2> {
    let parsed: syn::Path = syn::parse2(carrier_path.clone()).ok()?;
    let segs: Vec<_> = parsed.segments.iter().cloned().collect();
    if segs.is_empty() {
        return None;
    }
    let last_idx = segs.len() - 1;
    let last_seg = &segs[last_idx];
    let last_ident = last_seg.ident.to_string();
    // The carrier ident is `__ankyra_item_<kind>_<name>`. The sibling
    // const keeps the `<kind>_<name>` suffix verbatim — we only swap
    // the `__ankyra_item_` prefix for `__ANKYRA_FORMAT_` /
    // `__ANKYRA_VALUE_` / `__ANKYRA_NAME_`.
    let suffix = last_ident.strip_prefix("__ankyra_item_")?;
    // Strip an optional `<kind>_lt<N>_` prefix, leaving `<kind>_<name>`.
    // The lifetime-count suffix is emitted by
    // `ankyra-macros::shared::carrier_ident_with_lifetimes` only for
    // `reply`/`output` structs with lifetime parameters; the matching
    // sibling const uses the raw struct name without the `lt<N>_` infix,
    // so we normalise here before rebuilding the sibling path.
    let suffix = strip_lt_infix(suffix);
    let mut new_path = syn::Path {
        leading_colon: parsed.leading_colon,
        segments: syn::punctuated::Punctuated::default(),
    };
    for (i, seg) in segs.iter().enumerate() {
        if i < last_idx {
            new_path.segments.push(seg.clone());
        }
    }
    let new_ident = format!("{const_prefix}{suffix}");
    let new_ident: syn::Ident = syn::parse_str(&new_ident).ok()?;
    new_path.segments.push(syn::PathSegment {
        ident: new_ident,
        arguments: syn::PathArguments::None,
    });
    Some(quote!(#new_path))
}

/// Emit the `commands` section entries. Klipper uses message-format keys
/// for commands (the host builds command names from format strings).
///
/// Entry shape: `"identify offset=%u count=%u":1,` followed by user
/// commands in sort order.
fn emit_command_entries(args: &mut Vec<TokenStream2>, assembly: &Assembly) {
    // Collect the commands in their sorted order. The assembler's
    // canonical sort yields `identify` and user commands together; we
    // walk them in that order but special-case the synthesized
    // `identify` so its format string is the Klipper-accurate one.
    let commands: Vec<&AssembledItem> = assembly
        .items()
        .iter()
        .filter(|i| i.kind == "command")
        .collect();
    let last = commands.len().saturating_sub(1);
    for (idx, item) in commands.iter().enumerate() {
        push_literal(args, "\"");
        if item.name == IDENTIFY_CMD_NAME {
            push_literal(args, identify_cmd_format());
        } else {
            push_command_format(args, item);
        }
        push_literal(args, "\":");
        push_id(args, item.id);
        if idx != last {
            push_literal(args, ",");
        }
    }
}

/// Emit the `responses` section entries.
///
/// Shape per entry: `"<name>":[<id>,"<format>"]` for replies. The two
/// synthesized replies (`identify_response`, `shutdown`) carry their
/// Klipper-accurate formats because we own them.
fn emit_reply_entries(args: &mut Vec<TokenStream2>, assembly: &Assembly) {
    let replies: Vec<&AssembledItem> = assembly
        .items()
        .iter()
        .filter(|i| i.kind == "reply")
        .collect();
    let last = replies.len().saturating_sub(1);
    for (idx, item) in replies.iter().enumerate() {
        push_literal(args, "\"");
        // User replies route through the NAME sibling const so the wire
        // name reflects `#[klipper_reply]`'s PascalCase → snake_case
        // conversion. Synthesized reserved replies carry their exact
        // (already-snake_case) names as string literals.
        match item.name {
            IDENTIFY_RESPONSE_REPLY_NAME | SHUTDOWN_REPLY_NAME => push_literal(args, item.name),
            _ => push_item_name(args, item),
        }
        push_literal(args, "\":[");
        push_id(args, item.id);
        push_literal(args, ",\"");
        match item.name {
            IDENTIFY_RESPONSE_REPLY_NAME => push_literal(args, identify_response_reply_format()),
            SHUTDOWN_REPLY_NAME => push_literal(args, shutdown_reply_format()),
            _ => push_format(args, item),
        }
        push_literal(args, "\"]");
        if idx != last {
            push_literal(args, ",");
        }
    }
}

/// Emit the `output` section entries.
///
/// Shape matches the reply section: `"<name>":[<id>,"<format>"]`.
fn emit_output_entries(args: &mut Vec<TokenStream2>, assembly: &Assembly) {
    let outputs: Vec<&AssembledItem> = assembly
        .items()
        .iter()
        .filter(|i| i.kind == "output")
        .collect();
    let last = outputs.len().saturating_sub(1);
    for (idx, item) in outputs.iter().enumerate() {
        push_literal(args, "\"");
        // Dict key routes through the NAME sibling const so the
        // auto-converted wire name (PascalCase → snake_case) shows up
        // rather than the raw struct ident the carrier suffix carries.
        push_item_name(args, item);
        push_literal(args, "\":[");
        push_id(args, item.id);
        push_literal(args, ",\"");
        push_format(args, item);
        push_literal(args, "\"]");
        if idx != last {
            push_literal(args, ",");
        }
    }
}

/// Emit the `config` section: one entry per `#[klipper_constant]`. The
/// sibling `pub const __ANKYRA_VALUE_<name>` returns a JSON-ready value
/// (bare number or quoted string) so we splice it in directly without
/// extra quoting.
fn emit_constant_entries(args: &mut Vec<TokenStream2>, definitions: &[DefinitionInput]) {
    let constants: Vec<&DefinitionInput> = definitions
        .iter()
        .filter(|d| d.kind == DefinitionKind::Constant)
        .collect();
    let last = constants.len().saturating_sub(1);
    for (idx, def) in constants.iter().enumerate() {
        push_literal(args, "\"");
        push_definition_name(args, def);
        push_literal(args, "\":");
        push_definition_value(args, def, "inline-constant");
        if idx != last {
            push_literal(args, ",");
        }
    }
}

/// Emit the `enumerations` section: one entry per `klipper_enumeration!`.
/// The sibling `pub const __ANKYRA_VALUE_<name>` returns a pre-rendered
/// JSON object like `{"bldc_motor":0,"stepper":1}` so we splice it in
/// directly.
fn emit_enumeration_entries(args: &mut Vec<TokenStream2>, definitions: &[DefinitionInput]) {
    let enums: Vec<&DefinitionInput> = definitions
        .iter()
        .filter(|d| d.kind == DefinitionKind::Enumeration)
        .collect();
    let last = enums.len().saturating_sub(1);
    for (idx, def) in enums.iter().enumerate() {
        push_literal(args, "\"");
        push_definition_name(args, def);
        push_literal(args, "\":");
        push_definition_value(args, def, "{}");
        if idx != last {
            push_literal(args, ",");
        }
    }
}

/// Push an expression that evaluates to the protocol-facing name string for
/// a constant/enumeration definition.
///
/// Mirrors [`push_definition_value`]: when `def.module_prefix` is
/// `Some(prefix)`, the sibling `__ANKYRA_NAME_<kind>_<name>` const lives at
/// `<prefix>`. Otherwise we derive it from the carrier macro path's trailing
/// segment. Inline fixtures fall back to the parsed definition name.
fn push_definition_name(args: &mut Vec<TokenStream2>, def: &DefinitionInput) {
    if let Some(prefix) = &def.module_prefix {
        let const_ident_str = format!("__ANKYRA_NAME_{}_{}", def.kind_tag(), def.name);
        let const_ident: syn::Ident = syn::parse_str(&const_ident_str)
            .expect("__ANKYRA_NAME_<kind>_<name> is always a valid ident");
        args.push(quote!(#prefix::#const_ident));
        return;
    }
    if let Some(path) = &def.carrier_path {
        if let Some(const_path) = sibling_const_path(path, "__ANKYRA_NAME_") {
            args.push(const_path);
            return;
        }
    }
    let name_lit = Literal::string(&def.name);
    args.push(quote!(#name_lit));
}

/// Push an expression that evaluates to the JSON-ready value string for
/// a constant/enumeration definition. Falls back to `inline_default` as a
/// string literal when no carrier path is available (inline-tuple
/// fixtures used by tests).
///
/// # Path resolution strategy
///
/// Mirrors [`push_format`]: when `def.module_prefix` is `Some(prefix)`, the
/// `__ANKYRA_VALUE_<kind>_<name>` const lives at `<prefix>` (the module
/// where `#[klipper_constant]` / `klipper_enumeration!` emitted it), not at
/// the crate root where the carrier macro is `#[macro_export]`-hoisted.
/// When `module_prefix` is `None`, we fall back to the carrier-path
/// trailing-segment rewrite, which remains correct for crate-root items.
fn push_definition_value(
    args: &mut Vec<TokenStream2>,
    def: &DefinitionInput,
    inline_default: &str,
) {
    // Submodule definitions: VALUE const lives at the module where
    // #[klipper_constant] / klipper_enumeration! emitted it.
    if let Some(prefix) = &def.module_prefix {
        let const_ident_str = format!("__ANKYRA_VALUE_{}_{}", def.kind_tag(), def.name);
        let const_ident: syn::Ident = syn::parse_str(&const_ident_str)
            .expect("__ANKYRA_VALUE_<kind>_<name> is always a valid ident");
        args.push(quote!(#prefix::#const_ident));
        return;
    }
    // Bare-carrier fallback (crate-root items): derive the VALUE path from
    // the carrier macro path's trailing segment.
    if let Some(path) = &def.carrier_path {
        if let Some(const_path) = sibling_const_path(path, "__ANKYRA_VALUE_") {
            args.push(const_path);
            return;
        }
    }
    let value = if def.value_or_format.is_empty() {
        inline_default.to_string()
    } else {
        def.value_or_format.clone()
    };
    let value_lit = Literal::string(&value);
    args.push(quote!(#value_lit));
}

/// Emit the `static_strings` section: `"<id>":"<content>"` pairs keyed
/// by the assembler-assigned u16 id.
fn emit_static_string_entries(args: &mut Vec<TokenStream2>, assembly: &Assembly) {
    let strings = assembly.static_strings();
    let last = strings.len().saturating_sub(1);
    for (idx, (content, id)) in strings.iter().enumerate() {
        push_literal(args, "\"");
        let id_str = id.to_string();
        push_literal(args, &id_str);
        push_literal(args, "\":\"");
        // Static-string content is user-supplied UTF-8; escape JSON
        // specials before splicing.
        push_literal(args, &json_escape(content));
        push_literal(args, "\"");
        if idx != last {
            push_literal(args, ",");
        }
    }
}

/// Push the command message format. Identical to [`push_format`] — the
/// function is kept as a single name for the command section so the
/// emission code mirrors the other sections one-for-one.
fn push_command_format(args: &mut Vec<TokenStream2>, item: &AssembledItem) {
    push_format(args, item);
}

/// Minimal JSON string-content escaper. Matches the behaviour used by
/// the macros emitting `(value)` arms — the six mandatory escapes plus
/// `\u00XX` for other control bytes; non-ASCII bytes pass through as
/// valid UTF-8.
fn json_escape(s: &str) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                // `write!` into a `String` cannot fail; unwrap is safe.
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod module_prefix_value_tests {
    use super::*;
    use crate::input::{DefinitionInput, DefinitionKind};

    /// Build a `DefinitionInput` for a submodule constant with the given
    /// `module_prefix`. The carrier path uses `::mycrate::__ankyra_item_constant_<name>`
    /// so the fallback branch of `push_definition_value` can parse it with
    /// `syn::parse2` if needed.
    fn wrapped_constant(name: &str, prefix: Option<proc_macro2::TokenStream>) -> DefinitionInput {
        let carrier_ident = format!("__ankyra_item_constant_{name}");
        let carrier_ident: proc_macro2::Ident = syn::parse_str(&carrier_ident).unwrap();
        DefinitionInput {
            kind: DefinitionKind::Constant,
            name: name.into(),
            value_or_format: String::new(),
            descriptor_path: quote::quote!(::mycrate::#carrier_ident),
            carrier_path: Some(quote::quote!(::mycrate::#carrier_ident)),
            module_prefix: prefix,
        }
    }

    /// Build a `DefinitionInput` for a submodule enumeration.
    fn wrapped_enumeration(
        name: &str,
        prefix: Option<proc_macro2::TokenStream>,
    ) -> DefinitionInput {
        let carrier_ident = format!("__ankyra_item_enumeration_{name}");
        let carrier_ident: proc_macro2::Ident = syn::parse_str(&carrier_ident).unwrap();
        DefinitionInput {
            kind: DefinitionKind::Enumeration,
            name: name.into(),
            value_or_format: String::new(),
            descriptor_path: quote::quote!(::mycrate::#carrier_ident),
            carrier_path: Some(quote::quote!(::mycrate::#carrier_ident)),
            module_prefix: prefix,
        }
    }

    #[test]
    fn push_definition_value_prefers_module_prefix_for_constant() {
        let mut args: Vec<TokenStream2> = Vec::new();
        // Submodule constant: VALUE const lives at `$crate::sub`, not crate root.
        let def = wrapped_constant("MCU_FREQ", Some(quote::quote!($crate::sub)));
        push_definition_value(&mut args, &def, "0");
        let rendered = args[0].to_string().replace(' ', "");
        assert_eq!(
            rendered, "$crate::sub::__ANKYRA_VALUE_constant_MCU_FREQ",
            "submodule constant's VALUE const must resolve at its module"
        );
    }

    #[test]
    fn push_definition_value_prefers_module_prefix_for_enumeration() {
        let mut args: Vec<TokenStream2> = Vec::new();
        // Submodule enumeration: VALUE const lives at `$crate::sub`, not crate root.
        let def = wrapped_enumeration("motor_kind", Some(quote::quote!($crate::sub)));
        push_definition_value(&mut args, &def, "{}");
        let rendered = args[0].to_string().replace(' ', "");
        assert_eq!(
            rendered, "$crate::sub::__ANKYRA_VALUE_enumeration_motor_kind",
            "submodule enumeration's VALUE const must resolve at its module"
        );
    }

    #[test]
    fn push_definition_value_falls_back_for_crate_root_constant() {
        let mut args: Vec<TokenStream2> = Vec::new();
        // Crate-root constant: no module_prefix, so derive from carrier path.
        let def = wrapped_constant("CLOCK_FREQ", None);
        push_definition_value(&mut args, &def, "0");
        let rendered = args[0].to_string().replace(' ', "");
        assert_eq!(
            rendered, "::mycrate::__ANKYRA_VALUE_constant_CLOCK_FREQ",
            "crate-root constant falls back to carrier-path rewrite"
        );
    }

    #[test]
    fn push_definition_value_falls_back_for_crate_root_enumeration() {
        let mut args: Vec<TokenStream2> = Vec::new();
        // Crate-root enumeration: no module_prefix, so derive from carrier path.
        let def = wrapped_enumeration("pin_name", None);
        push_definition_value(&mut args, &def, "{}");
        let rendered = args[0].to_string().replace(' ', "");
        assert_eq!(
            rendered, "::mycrate::__ANKYRA_VALUE_enumeration_pin_name",
            "crate-root enumeration falls back to carrier-path rewrite"
        );
    }

    #[test]
    fn emit_enumeration_entries_uses_exported_name_const() {
        let mut args: Vec<TokenStream2> = Vec::new();
        // Renamed enumerations carry the Rust ident (`MotorKind`) in the
        // carrier macro name but expose the protocol-facing name through
        // `__ANKYRA_NAME_enumeration_MotorKind`.
        let def = wrapped_enumeration("MotorKind", Some(quote::quote!($crate::sub)));
        emit_enumeration_entries(&mut args, &[def]);
        let rendered = args
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>()
            .join(" ")
            .replace(' ', "");
        assert!(
            rendered.contains("$crate::sub::__ANKYRA_NAME_enumeration_MotorKind"),
            "enumeration key must come from the exported-name const: {rendered}"
        );
    }
}

#[cfg(test)]
mod module_prefix_format_tests {
    use super::*;
    use crate::sort::{ItemInput, ItemKind, assemble};

    /// Build an `ItemInput` representing a submodule command item for
    /// testing `push_format`'s prefix-aware path logic. Only the fields
    /// `push_format` actually reads are populated meaningfully.
    ///
    /// The carrier path uses `::mycrate::__ankyra_item_command_<name>`
    /// (a real absolute path, not `$crate::…`) so that `sibling_const_path`
    /// can parse it with `syn::parse2` in the crate-root fallback branch.
    fn wrapped_command_item(
        name: &'static str,
        prefix: Option<proc_macro2::TokenStream>,
    ) -> ItemInput {
        let carrier_ident = quote::format_ident!("__ankyra_item_command_{name}");
        ItemInput {
            kind: ItemKind::Command,
            name: name.into(),
            lifetime_count: 0,
            message_format: None,
            descriptor_path: None,
            dispatch_path: None,
            carrier_path: Some(quote::quote!(::mycrate::#carrier_ident)),
            module_prefix: prefix,
        }
    }

    #[test]
    fn push_format_prefers_module_prefix() {
        let mut args: Vec<TokenStream2> = Vec::new();
        // Submodule item: carrier macro is hoisted to crate root but FORMAT
        // const lives at `$crate::sub`. The module_prefix directs push_format
        // to emit `$crate::sub::__ANKYRA_FORMAT_command_foo` directly.
        let item = wrapped_command_item("foo", Some(quote::quote!($crate::sub)));
        let assembly = assemble(vec![item], Vec::<String>::new()).expect("assemble succeeds");
        let foo = assembly
            .items()
            .iter()
            .find(|it| it.name == "foo")
            .expect("foo present");
        push_format(&mut args, foo);
        let rendered = args[0].to_string().replace(' ', "");
        assert_eq!(
            rendered, "$crate::sub::__ANKYRA_FORMAT_command_foo",
            "submodule item's FORMAT const must resolve at its module"
        );
    }

    #[test]
    fn push_format_falls_back_to_carrier_rewrite_for_crate_root() {
        let mut args: Vec<TokenStream2> = Vec::new();
        // Crate-root item: no module_prefix, so push_format derives the path
        // from the carrier macro path by swapping the trailing `__ankyra_item_`
        // prefix for `__ANKYRA_FORMAT_`.
        let item = wrapped_command_item("bar", None);
        let assembly = assemble(vec![item], Vec::<String>::new()).expect("assemble succeeds");
        let bar = assembly
            .items()
            .iter()
            .find(|it| it.name == "bar")
            .expect("bar present");
        push_format(&mut args, bar);
        let rendered = args[0].to_string().replace(' ', "");
        assert_eq!(
            rendered, "::mycrate::__ANKYRA_FORMAT_command_bar",
            "crate-root item falls back to carrier-path rewrite"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sort::{ItemInput, assemble};

    fn assembly_of(items: Vec<ItemInput>, strings: Vec<String>) -> Assembly {
        assemble(items, strings).unwrap()
    }

    #[test]
    fn concatcp_args_open_with_commands_section() {
        let a = assembly_of(vec![], vec![]);
        let args = build_concatcp_args(&a, &[]);
        let rendered = args
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>()
            .join(" | ");
        // First fragment must open the JSON object and the commands
        // section; the identify command immediately follows.
        assert!(
            rendered.starts_with("\"{\\\"commands\\\":{\""),
            "first fragment did not open with commands section: {rendered}"
        );
    }

    #[test]
    fn identify_and_shutdown_present_for_empty_input() {
        let a = assembly_of(vec![], vec![]);
        let args = build_concatcp_args(&a, &[]);
        let rendered = args
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        // Each section header must appear in the emitted fragments.
        assert!(rendered.contains("commands"), "commands header missing");
        assert!(rendered.contains("responses"), "responses header missing");
        assert!(rendered.contains("output"), "output header missing");
        assert!(
            rendered.contains("enumerations"),
            "enumerations header missing"
        );
        assert!(
            rendered.contains("identify offset=%u count=%u"),
            "identify format missing"
        );
        assert!(
            rendered.contains("identify_response offset=%u data=%.*s"),
            "identify_response format missing"
        );
        assert!(
            rendered.contains("shutdown clock=%u static_string_id=%hu"),
            "shutdown format missing"
        );
    }

    #[test]
    fn static_string_entry_escapes_embedded_quote() {
        let a = assembly_of(vec![], vec!["he said \"hi\"".to_string()]);
        let args = build_concatcp_args(&a, &[]);
        let rendered = args
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        // `json_escape` must emit backslash-quote for the inner quote.
        assert!(
            rendered.contains("he said \\\\\\\"hi\\\\\\\""),
            "static string not escaped: {rendered}"
        );
    }
}
