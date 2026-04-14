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
//!                  "<fmt>": <id>, ... },
//!   "output":    { "<fmt>": <id>, ... },
//!   "config":    { "CLOCK_FREQ": 168000000, "MCU": "stm32f407", ... },
//!   "enumerations": { "motor_kind": {"bldc_motor": 0}, ...,
//!                     "static_string_id": {"boom": 2, ...} },
//!   "static_strings": { "<id>": "<text>", ... },
//!   "version":        "ankyra-v0.1",
//!   "build_versions": "ankyra-0.1.0",
//!   "app":            "ankyra",
//!   "license":        "MIT OR Apache-2.0"
//! }
//! ```
//!
//! The four trailer fields (`version`, `build_versions`, `app`,
//! `license`) are configurable via `ankyra_config!`. Omitting a key
//! falls through to the ankyra default shown above, so downstream
//! firmware is not forced to opt in. See [`TrailerMetadata`].
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
//! (and `__ANKYRA_VALUE_<kind>_<name>` / `__ANKYRA_NAME_<kind>_<name>` for
//! constants/enumerations) next to its descriptor fn. The assembler
//! receives an explicit `sibling_scope` for every carrier-backed item
//! via the wrapped-carrier form (`{ prefix: (…), carrier!() }`), and
//! constructs `<scope>::__ANKYRA_<KIND>_<kind>_<name>` directly. At
//! const-eval time rustc substitutes each path with its stringified
//! value and `concatcp!` stitches the full JSON document.
//!
//! An earlier revision reconstructed the path by parsing the carrier
//! macro's token stream and rewriting its trailing segment. That design
//! silently fell back to inline defaults when resolution failed — so a
//! provider listing a renamed or deleted constant produced a
//! valid-looking but empty config entry. The `sibling_scope` field
//! replaces that rewrite; missing scope on a carrier-backed definition
//! now emits `compile_error!` instead of silently degrading.
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

/// User-supplied overrides for the dictionary's trailer metadata fields.
///
/// Each field holds the raw `&'static str`-valued expression the user
/// passed to `ankyra_config!` (a string literal, `env!(...)`, a module
/// path constant, etc.). `None` means the user omitted that key and the
/// corresponding ankyra default (`"ankyra"`, `"ankyra-v0.1"`, etc.)
/// should be spliced in instead.
///
/// Kept in its own struct rather than four separate parameters so
/// `emit`'s signature stays readable as more dictionary-shape knobs are
/// added.
pub(crate) struct TrailerMetadata {
    pub app: Option<TokenStream2>,
    pub version: Option<TokenStream2>,
    pub build_versions: Option<TokenStream2>,
    pub license: Option<TokenStream2>,
}

/// Emit the `__ANKYRA_DICT` string constant and the `DICT_BYTES` slice.
///
/// The caller splices this output into the `_ankyra_config` module tree.
pub(crate) fn emit(
    assembly: &Assembly,
    definitions: &[DefinitionInput],
    metadata: &TrailerMetadata,
) -> TokenStream2 {
    let fragments = build_concatcp_args(assembly, definitions);

    // Bind every metadata override to a typed `pub const &'static str`
    // before splicing it into `concatcp!`. The type ascription gives
    // rustc a single, controlled site to reject non-`&str` expressions
    // (e.g. `app = 42`) with a span-accurate diagnostic; `concatcp!`'s
    // own error would otherwise cite the macro internals. Defaults are
    // string literals, so their `const` binding is trivially valid.
    let app_default: &str = "ankyra";
    let version_default: &str = "ankyra-v0.1";
    // Ankyra-assemble's own package version, resolved at this crate's
    // compile site so it stays "ankyra-<ankyra-version>" regardless of
    // which crate invoked `ankyra_config!`.
    let build_versions_default: &str = concat!("ankyra-", env!("CARGO_PKG_VERSION"));
    let license_default: &str = "MIT OR Apache-2.0";

    let app_expr = metadata.app.clone().unwrap_or_else(|| quote!(#app_default));
    let version_expr = metadata
        .version
        .clone()
        .unwrap_or_else(|| quote!(#version_default));
    let build_versions_expr = metadata
        .build_versions
        .clone()
        .unwrap_or_else(|| quote!(#build_versions_default));
    let license_expr = metadata
        .license
        .clone()
        .unwrap_or_else(|| quote!(#license_default));

    quote! {
        /// Data dictionary `"app"` trailer field. User-supplied via
        /// `ankyra_config! { app = ... }` or the `"ankyra"` default.
        #[doc(hidden)]
        pub const __ANKYRA_META_APP: &'static str = #app_expr;

        /// Data dictionary `"version"` trailer field. User-supplied via
        /// `ankyra_config! { version = ... }` or the `"ankyra-v0.1"`
        /// default.
        #[doc(hidden)]
        pub const __ANKYRA_META_VERSION: &'static str = #version_expr;

        /// Data dictionary `"build_versions"` trailer field.
        /// User-supplied via `ankyra_config! { build_versions = ... }`
        /// or the `concat!("ankyra-", env!("CARGO_PKG_VERSION"))`
        /// default evaluated at ankyra-assemble's compile site.
        #[doc(hidden)]
        pub const __ANKYRA_META_BUILD_VERSIONS: &'static str = #build_versions_expr;

        /// Data dictionary `"license"` trailer field. User-supplied via
        /// `ankyra_config! { license = ... }` or the
        /// `"MIT OR Apache-2.0"` default.
        #[doc(hidden)]
        pub const __ANKYRA_META_LICENSE: &'static str = #license_expr;

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
    emit_enumeration_entries(&mut args, assembly, definitions);
    push_literal(&mut args, "},\"static_strings\":{");
    emit_static_string_entries(&mut args, assembly);
    // Trailer: configurable metadata fields. Each `__ANKYRA_META_*`
    // const is a `pub const &'static str` defined in the same emitted
    // module (see `emit`), holding either the user's override from
    // `ankyra_config!` or ankyra's default. `concatcp!` substitutes the
    // const's value at its own call site, so the JSON trailer's shape
    // ("version":"...","build_versions":"...","app":"...","license":"...")
    // matches the pre-refactor output byte-for-byte when no overrides
    // are supplied.
    push_literal(&mut args, "},\"version\":\"");
    args.push(quote!(__ANKYRA_META_VERSION));
    push_literal(&mut args, "\",\"build_versions\":\"");
    args.push(quote!(__ANKYRA_META_BUILD_VERSIONS));
    push_literal(&mut args, "\",\"app\":\"");
    args.push(quote!(__ANKYRA_META_APP));
    push_literal(&mut args, "\",\"license\":\"");
    args.push(quote!(__ANKYRA_META_LICENSE));
    push_literal(&mut args, "\"}");
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
/// item's message format.
///
/// # Path resolution strategy
///
/// Every carrier-backed item carries an explicit `sibling_scope` — the
/// module where `#[klipper_*]` emitted its sibling
/// `pub const __ANKYRA_FORMAT_<kind>_<name>`. We build that path
/// directly (`<scope>::__ANKYRA_FORMAT_<kind>_<name>`). Crate-root items
/// flow through with `sibling_scope = Some($crate)`; submodule items
/// with `Some($crate::submod)`.
///
/// When `item.sibling_scope` is `None` the item was constructed without a
/// carrier — either a synthesized reserved entry intercepted by
/// [`emit_reply_entries`] before it reaches `push_format`, or an
/// inline-tuple test fixture that supplies its own `message_format`.
/// Those paths fall back to the embedded-literal branch at the bottom of
/// the function.
///
/// Using a `pub const` path (rather than invoking the carrier macro
/// directly) sidesteps rust-lang/rust#52234: same-crate
/// `#[macro_export]` macros cannot be referred to by absolute paths,
/// but `pub const` items can.
///
/// The prior carrier-path trailing-segment rewrite (parse the carrier
/// macro path, swap the last ident) is gone for carrier-backed items.
/// It silently fell back to inline defaults when resolution failed,
/// hiding mistakes like a provider listing a renamed or deleted const.
fn push_format(args: &mut Vec<TokenStream2>, item: &AssembledItem) {
    if let Some(scope) = &item.sibling_scope {
        let const_ident_str = format!("__ANKYRA_FORMAT_{}_{}", item.kind, item.name);
        let const_ident: syn::Ident = syn::parse_str(&const_ident_str)
            .expect("__ANKYRA_FORMAT_<kind>_<name> is always a valid ident");
        args.push(quote!(#scope::#const_ident));
        return;
    }
    // Inline-fixture fallback: items without a sibling_scope also have no
    // carrier, so embed `message_format` (or the item name as a last
    // resort) as a string literal. Production items always reach the
    // scope branch above.
    let fmt = item
        .message_format
        .as_deref()
        .unwrap_or(item.name)
        .to_string();
    let fmt_lit = Literal::string(&fmt);
    args.push(quote!(#fmt_lit));
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
/// Shape per entry: `"<format>":<id>` — matching commands. Klipper's
/// `msgproto.py::_init_messages` iterates the merged
/// `commands ∪ responses ∪ output` dict and VLQ-encodes the value as the
/// message id, so the value must be a scalar int. The protocol-facing
/// name is implicit in the format string's first token (Klipper derives
/// it there). The two synthesized replies (`identify_response`,
/// `shutdown`) carry their Klipper-accurate formats because we own them.
fn emit_reply_entries(args: &mut Vec<TokenStream2>, assembly: &Assembly) {
    let replies: Vec<&AssembledItem> = assembly
        .items()
        .iter()
        .filter(|i| i.kind == "reply")
        .collect();
    let last = replies.len().saturating_sub(1);
    for (idx, item) in replies.iter().enumerate() {
        push_literal(args, "\"");
        match item.name {
            IDENTIFY_RESPONSE_REPLY_NAME => push_literal(args, identify_response_reply_format()),
            SHUTDOWN_REPLY_NAME => push_literal(args, shutdown_reply_format()),
            _ => push_format(args, item),
        }
        push_literal(args, "\":");
        push_id(args, item.id);
        if idx != last {
            push_literal(args, ",");
        }
    }
}

/// Emit the `output` section entries.
///
/// Shape matches the command/reply sections: `"<format>":<id>`. See
/// [`emit_reply_entries`] for why the value must be a scalar int.
fn emit_output_entries(args: &mut Vec<TokenStream2>, assembly: &Assembly) {
    let outputs: Vec<&AssembledItem> = assembly
        .items()
        .iter()
        .filter(|i| i.kind == "output")
        .collect();
    let last = outputs.len().saturating_sub(1);
    for (idx, item) in outputs.iter().enumerate() {
        push_literal(args, "\"");
        push_format(args, item);
        push_literal(args, "\":");
        push_id(args, item.id);
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
///
/// Klipper also exposes static strings through a synthesized
/// `static_string_id` enumeration keyed by string content. Keep ankyra's
/// top-level `static_strings` section for direct macro resolution, but
/// also emit this enumeration for dictionary-shape parity.
fn emit_enumeration_entries(
    args: &mut Vec<TokenStream2>,
    assembly: &Assembly,
    definitions: &[DefinitionInput],
) {
    let enums: Vec<&DefinitionInput> = definitions
        .iter()
        .filter(|d| d.kind == DefinitionKind::Enumeration)
        .collect();
    for (idx, def) in enums.iter().enumerate() {
        push_literal(args, "\"");
        push_definition_name(args, def);
        push_literal(args, "\":");
        push_definition_value(args, def, "{}");
        if idx + 1 != enums.len() || !assembly.static_strings().is_empty() {
            push_literal(args, ",");
        }
    }
    emit_static_string_id_enumeration(args, assembly);
}

/// Emit Klipper's `static_string_id` enumeration from the assembler's
/// static-string table. The entry shape is:
/// `"static_string_id":{"content":<id>,...}`.
fn emit_static_string_id_enumeration(args: &mut Vec<TokenStream2>, assembly: &Assembly) {
    let strings = assembly.static_strings();
    if strings.is_empty() {
        return;
    }

    push_literal(args, "\"static_string_id\":{");
    let last = strings.len().saturating_sub(1);
    for (idx, (content, id)) in strings.iter().enumerate() {
        push_literal(args, "\"");
        push_literal(args, &json_escape(content));
        push_literal(args, "\":");
        push_id(args, *id);
        if idx != last {
            push_literal(args, ",");
        }
    }
    push_literal(args, "}");
}

/// Push an expression that evaluates to the protocol-facing name string for
/// a constant/enumeration definition.
///
/// Mirrors [`push_definition_value`]. Every carrier-backed definition
/// reaches the assembler with an explicit `sibling_scope` — the
/// `#[macro_export]`-hoisted carrier macro lives at the crate root
/// regardless of where the user wrote `#[klipper_constant]`, but the
/// matching `pub const __ANKYRA_NAME_<kind>_<name>` is co-located with
/// the item itself. We splice `<scope>::__ANKYRA_NAME_<kind>_<name>`
/// directly.
///
/// If a definition has a `carrier_path` but no `sibling_scope`, something
/// went wrong in the parser: a provider listed this const but we cannot
/// resolve its sibling. Emit a `compile_error!` rather than silently
/// falling back to inline text — the original bug this patch addresses
/// was that a missing/renamed `__ANKYRA_NAME_*` const would surface as a
/// valid-looking but empty config dictionary entry.
///
/// Inline fixtures (`carrier_path = None`, `sibling_scope = None`) fall
/// back to the parsed definition name as a string literal — that branch
/// is exercised only by test utilities.
fn push_definition_name(args: &mut Vec<TokenStream2>, def: &DefinitionInput) {
    if let Some(scope) = &def.sibling_scope {
        let const_ident_str = format!("__ANKYRA_NAME_{}_{}", def.kind_tag(), def.name);
        let const_ident: syn::Ident = syn::parse_str(&const_ident_str)
            .expect("__ANKYRA_NAME_<kind>_<name> is always a valid ident");
        args.push(quote!(#scope::#const_ident));
        return;
    }
    if def.carrier_path.is_some() {
        // Hard error: a carrier is registered but no scope was threaded
        // through. Previously this silently produced an empty/bogus dict
        // entry; we surface a concrete compile error instead so providers
        // that reference a stale or renamed constant fail loudly.
        let msg = format!(
            "ankyra: cannot resolve sibling const `__ANKYRA_NAME_{}_{}` — \
             the definition reached the assembler without a sibling scope. \
             This usually means the `#[klipper_{}]` item was renamed, \
             deleted, or not re-exported at the path the provider lists.",
            def.kind_tag(),
            def.name,
            def.kind_tag(),
        );
        let lit = Literal::string(&msg);
        args.push(quote!(::core::compile_error!(#lit)));
        return;
    }
    let name_lit = Literal::string(&def.name);
    args.push(quote!(#name_lit));
}

/// Push an expression that evaluates to the JSON-ready value string for
/// a constant/enumeration definition.
///
/// # Path resolution strategy
///
/// Mirrors [`push_format`]: every carrier-backed definition reaches the
/// assembler with an explicit `sibling_scope`, so we splice
/// `<scope>::__ANKYRA_VALUE_<kind>_<name>` directly — crate-root items
/// with `$crate`, submodule items with `$crate::submod`.
///
/// If a definition has a `carrier_path` but no `sibling_scope`, something
/// went wrong in the parser. Emit `compile_error!` rather than silently
/// falling back to the inline default — an unregistered or renamed
/// constant must fail the build, not disappear into an empty config
/// entry.
///
/// Inline fixtures (no carrier, no scope) use `inline_default` as a
/// string literal; that branch exists only for test utilities.
fn push_definition_value(
    args: &mut Vec<TokenStream2>,
    def: &DefinitionInput,
    inline_default: &str,
) {
    if let Some(scope) = &def.sibling_scope {
        let const_ident_str = format!("__ANKYRA_VALUE_{}_{}", def.kind_tag(), def.name);
        let const_ident: syn::Ident = syn::parse_str(&const_ident_str)
            .expect("__ANKYRA_VALUE_<kind>_<name> is always a valid ident");
        args.push(quote!(#scope::#const_ident));
        return;
    }
    if def.carrier_path.is_some() {
        let msg = format!(
            "ankyra: cannot resolve sibling const `__ANKYRA_VALUE_{}_{}` — \
             the definition reached the assembler without a sibling scope. \
             This usually means the `#[klipper_{}]` item was renamed, \
             deleted, or not re-exported at the path the provider lists.",
            def.kind_tag(),
            def.name,
            def.kind_tag(),
        );
        let lit = Literal::string(&msg);
        args.push(quote!(::core::compile_error!(#lit)));
        return;
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
mod sibling_scope_value_tests {
    use super::*;
    use crate::input::{DefinitionInput, DefinitionKind};
    use crate::sort::{ItemInput, assemble};

    /// Build a `DefinitionInput` for a submodule constant with the given
    /// `sibling_scope`. The carrier path is kept populated to mirror the
    /// real `DefinitionInput` shape — since the carrier-path rewrite
    /// fallback has been removed, the tests only rely on `sibling_scope`
    /// for the success case and on `carrier_path.is_some()` for the
    /// hard-error case.
    fn wrapped_constant(name: &str, prefix: Option<proc_macro2::TokenStream>) -> DefinitionInput {
        let carrier_ident = format!("__ankyra_item_constant_{name}");
        let carrier_ident: proc_macro2::Ident = syn::parse_str(&carrier_ident).unwrap();
        DefinitionInput {
            kind: DefinitionKind::Constant,
            name: name.into(),
            value_or_format: String::new(),
            descriptor_path: quote::quote!(::mycrate::#carrier_ident),
            carrier_path: Some(quote::quote!(::mycrate::#carrier_ident)),
            sibling_scope: prefix,
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
            sibling_scope: prefix,
        }
    }

    #[test]
    fn push_definition_value_prefers_sibling_scope_for_constant() {
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
    fn push_definition_value_prefers_sibling_scope_for_enumeration() {
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
    fn push_definition_value_hard_errors_when_carrier_has_no_scope() {
        let mut args: Vec<TokenStream2> = Vec::new();
        // Carrier-backed constant with a missing sibling_scope used to
        // silently fall back to the carrier-path trailing-segment
        // rewrite, which then fell back to the inline default when
        // parsing failed. The new behaviour is a hard compile-time
        // error so providers that reference a stale/renamed const
        // never silently degrade to an empty config entry.
        let def = wrapped_constant("CLOCK_FREQ", None);
        push_definition_value(&mut args, &def, "0");
        let rendered = args[0].to_string();
        assert!(
            rendered.contains("compile_error"),
            "expected compile_error! for carrier-backed const without scope; got {rendered}"
        );
        assert!(
            rendered.contains("__ANKYRA_VALUE_constant_CLOCK_FREQ"),
            "compile_error message should name the missing const; got {rendered}"
        );
    }

    #[test]
    fn push_definition_value_hard_errors_for_enumeration_without_scope() {
        let mut args: Vec<TokenStream2> = Vec::new();
        let def = wrapped_enumeration("pin_name", None);
        push_definition_value(&mut args, &def, "{}");
        let rendered = args[0].to_string();
        assert!(
            rendered.contains("compile_error")
                && rendered.contains("__ANKYRA_VALUE_enumeration_pin_name"),
            "expected compile_error for carrier-backed enum without scope; got {rendered}"
        );
    }

    #[test]
    fn push_definition_value_inline_fixture_uses_default() {
        // Without a carrier_path the inline-default branch runs — this
        // is the path exercised by hand-authored tests that bypass the
        // wrapped-carrier parser entirely.
        let mut args: Vec<TokenStream2> = Vec::new();
        let def = DefinitionInput {
            kind: DefinitionKind::Constant,
            name: "INLINE".into(),
            value_or_format: String::new(),
            descriptor_path: quote::quote!(unused),
            carrier_path: None,
            sibling_scope: None,
        };
        push_definition_value(&mut args, &def, "42");
        assert_eq!(args[0].to_string(), "\"42\"");
    }

    #[test]
    fn emit_enumeration_entries_uses_exported_name_const() {
        let mut args: Vec<TokenStream2> = Vec::new();
        // Renamed enumerations carry the Rust ident (`MotorKind`) in the
        // carrier macro name but expose the protocol-facing name through
        // `__ANKYRA_NAME_enumeration_MotorKind`.
        let def = wrapped_enumeration("MotorKind", Some(quote::quote!($crate::sub)));
        let assembly =
            assemble(Vec::<ItemInput>::new(), Vec::<String>::new()).expect("assemble succeeds");
        emit_enumeration_entries(&mut args, &assembly, &[def]);
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

    #[test]
    fn emit_enumeration_entries_synthesizes_static_string_id() {
        let mut args: Vec<TokenStream2> = Vec::new();
        let assembly =
            assemble(Vec::<ItemInput>::new(), vec!["boom".to_string()]).expect("assemble succeeds");
        emit_enumeration_entries(&mut args, &assembly, &[]);
        let rendered = args
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>()
            .join(" ")
            .replace(' ', "");
        assert!(
            rendered.contains("static_string_id"),
            "static_string_id enumeration missing: {rendered}"
        );
        assert!(
            rendered.contains("boom") && rendered.contains("2u16"),
            "static_string_id enumeration must map content to assigned id: {rendered}"
        );
    }
}

#[cfg(test)]
mod sibling_scope_format_tests {
    use super::*;
    use crate::sort::{ItemInput, ItemKind, assemble};

    /// Build an `ItemInput` representing a submodule command item for
    /// testing `push_format`'s prefix-aware path logic. Only the fields
    /// `push_format` actually reads are populated meaningfully.
    ///
    /// The carrier path uses `::mycrate::__ankyra_item_command_<name>`
    /// (a real absolute path, not `$crate::…`) — legacy test artefact
    /// from the pre-`sibling_scope` era when the dictionary builder
    /// reconstructed the FORMAT const path by parsing this tokens stream.
    /// Today `push_format` reads the scope directly; the `carrier_path`
    /// field is kept for parity with the real `AssembledItem` shape.
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
            sibling_scope: prefix,
        }
    }

    #[test]
    fn push_format_prefers_sibling_scope() {
        let mut args: Vec<TokenStream2> = Vec::new();
        // Submodule item: carrier macro is hoisted to crate root but FORMAT
        // const lives at `$crate::sub`. The sibling_scope directs push_format
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
    fn push_format_inline_fallback_when_scope_absent() {
        let mut args: Vec<TokenStream2> = Vec::new();
        // No sibling_scope: push_format falls through to the inline
        // branch and embeds `message_format` (or the item name) as a
        // string literal. This covers the synthesized-reserved and
        // inline-fixture cases; production items always arrive with a
        // scope now.
        let item = wrapped_command_item("bar", None);
        let assembly = assemble(vec![item], Vec::<String>::new()).expect("assemble succeeds");
        let bar = assembly
            .items()
            .iter()
            .find(|it| it.name == "bar")
            .expect("bar present");
        push_format(&mut args, bar);
        let rendered = args[0].to_string();
        assert_eq!(
            rendered, "\"bar\"",
            "scope-less item falls back to inline name literal"
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
