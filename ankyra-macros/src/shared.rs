use proc_macro2::{Ident, Span};
use quote::format_ident;
use syn::punctuated::Punctuated;
use syn::{Error, Path};

pub fn descriptor_ident(name: &Ident) -> Ident {
    format_ident!("__ankyra_descriptor_{}", name)
}

/// Derive the Klipper wire name from a Rust item identifier.
///
/// Klipper wire names are conventionally `snake_case`, while Rust types are
/// conventionally `PascalCase`. Instead of forcing consumers to write
/// `pub struct trsync_state` with `#[allow(non_camel_case_types)]`, the
/// ankyra macros auto-convert `PascalCase` struct idents to `snake_case` at
/// expansion time.
///
/// The rule is deliberately one-directional: if the ident contains any
/// uppercase character, treat it as `PascalCase` / `camelCase` and convert to
/// `snake_case`. Otherwise return the input verbatim. This preserves
/// backward compatibility with any existing consumer that wrote a
/// lowercase struct ident (e.g. `pub struct trsync_state`).
///
/// # Conversion
///
/// * `TrsyncState` → `trsync_state`
/// * `ADCValue` → `adc_value` (consecutive uppercase collapses before
///   the next lowercase letter)
/// * `SPITransfer` → `spi_transfer`
/// * `HTTPStatus2xx` → `http_status2xx`
/// * `stats` → `stats` (already lowercase — returned verbatim)
/// * `already_with_underscores` → `already_with_underscores`
/// * `CLOCK_FREQ` → `CLOCK_FREQ` (`SCREAMING_SNAKE_CASE` — returned
///   verbatim, matching Klipper/anchor constant convention)
/// * `MCU` → `MCU` (single-word all-uppercase — returned verbatim)
///
/// At each uppercase letter (after the first), insert an underscore
/// when:
///
/// * the previous character is lowercase or a digit (camelCase
///   boundary: `MyFoo` → `my_foo`), or
/// * the previous character is uppercase and the next is lowercase
///   (acronym-to-word boundary: `HTTPFoo` → `http_foo`).
pub fn pascal_to_snake(ident: &str) -> String {
    // Already snake / all-lowercase: passthrough.
    if !ident.chars().any(char::is_uppercase) {
        return ident.to_string();
    }
    // SCREAMING_SNAKE_CASE (all uppercase + underscores + digits):
    // passthrough verbatim. Matches Klipper/anchor convention for
    // `#[klipper_constant]` idents — e.g. `CLOCK_FREQ` must appear in
    // the data dictionary's `config` section exactly as written because
    // Klipper's host looks it up via `get_constant_float("CLOCK_FREQ")`.
    if ident
        .chars()
        .all(|c| c.is_uppercase() || c == '_' || c.is_ascii_digit())
    {
        return ident.to_string();
    }
    let chars: Vec<char> = ident.chars().collect();
    let mut out = String::with_capacity(ident.len() + 4);
    for i in 0..chars.len() {
        let c = chars[i];
        if c.is_uppercase() {
            if i > 0 {
                let prev = chars[i - 1];
                let next = chars.get(i + 1).copied().unwrap_or('\0');
                if prev.is_lowercase()
                    || prev.is_numeric()
                    || (prev.is_uppercase() && next.is_lowercase())
                {
                    out.push('_');
                }
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

pub fn dispatch_ident(name: &Ident) -> Ident {
    format_ident!("__ankyra_dispatch_{}", name)
}

pub fn carrier_ident(kind: &str, name: &Ident) -> Ident {
    format_ident!("__ankyra_item_{}_{}", kind, name)
}

/// Lifetime-count-aware variant of [`carrier_ident`] for `#[klipper_reply]`
/// and `#[klipper_output]` structs. When the struct declares N > 0
/// lifetime parameters the ident includes an `_lt<N>` infix before the
/// name: `__ankyra_item_reply_lt1_FooReply`. For N = 0 the ident is
/// identical to [`carrier_ident`] to preserve the v0.1 carrier-name
/// convention.
///
/// The assembler's input parser detects the `_lt<N>_` infix and threads N
/// through to the `AssembledItem` so the `SendReply` / `SendOutput` impl
/// emission in `senders::emit` can synthesize the required
/// `impl<'a0, 'a1, ...>` header. Encoding the count in the ident (rather
/// than via a sibling `pub const` or a carrier-macro arm) is the only
/// mechanism that reaches the proc-macro at expansion time — `pub const`
/// values are resolved at rustc-eval time, and invoking a sibling carrier
/// arm in the same crate as the assembler trips rust-lang/rust#52234.
pub fn carrier_ident_with_lifetimes(kind: &str, name: &Ident, lifetime_count: usize) -> Ident {
    if lifetime_count == 0 {
        carrier_ident(kind, name)
    } else {
        format_ident!("__ankyra_item_{}_lt{}_{}", kind, lifetime_count, name)
    }
}

/// Sibling `pub const` carrying the Klipper-style message format for a
/// `#[klipper_command]` / `#[klipper_reply]` / `#[klipper_output]` item.
/// The assembler reconstructs this path from the carrier prefix and
/// splices it into the data dictionary via `const_format::concatcp!`.
///
/// The ident is kind-qualified so that a `#[klipper_command]` and a
/// `#[klipper_reply]` sharing the same protocol name (which sort-stage
/// dedup rejects later on but which the item-level macros expand
/// independently) do not collide at definition time. Using a `pub
/// const` (rather than invoking the carrier macro) lets us refer to
/// same-crate items via `crate::…` paths without tripping
/// rust-lang/rust#52234, which rejects absolute paths to
/// `#[macro_export]` macros from the same crate.
pub fn format_const_ident(kind: &str, name: &Ident) -> Ident {
    format_ident!("__ANKYRA_FORMAT_{}_{}", kind, name)
}

/// Sibling `pub const` carrying the JSON-ready value string for a
/// `#[klipper_constant]` / `klipper_enumeration!` item. Same reasoning
/// as [`format_const_ident`]: a `pub const` path sidesteps
/// rust-lang/rust#52234 in the same-crate case, and the kind qualifier
/// prevents collisions with similarly-named items of a different kind.
pub fn value_const_ident(kind: &str, name: &Ident) -> Ident {
    format_ident!("__ANKYRA_VALUE_{}_{}", kind, name)
}

/// Sibling `pub const` carrying the protocol-facing name for an item.
/// See [`format_const_ident`] for the path-reconstruction rationale.
pub fn name_const_ident(kind: &str, name: &Ident) -> Ident {
    format_ident!("__ANKYRA_NAME_{}_{}", kind, name)
}

pub fn provider_companion_ident(name: &Ident) -> Ident {
    format_ident!("__ankyra_provider_{}", name)
}

/// FNV-1a 64-bit hash with canonical offset basis and prime. Chosen over
/// `std::collections::hash_map::DefaultHasher` because `DefaultHasher` is
/// explicitly documented as not stable across rustc versions — which would
/// break build reproducibility for `__ANKYRA_SS_<hash>` symbol names across
/// rustc upgrades. FNV-1a is trivially implementable, dependency-free, and
/// deterministic by specification. Collisions are detected and rejected at
/// aggregation time.
pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// Build the static-string symbol name `__ANKYRA_SS_<hash>` for a given
/// literal. Both the `klipper_static_string!` proc-macro and the assembler
/// call this same function, guaranteeing agreement on symbol names.
pub fn static_string_hash_ident(msg: &str, span: Span) -> Ident {
    let hash = fnv1a_64(msg.as_bytes());
    format_ident!("__ANKYRA_SS_{:016x}", hash, span = span)
}

/// Rewrite `a::b::c::P` into the companion-macro path the CPS fold can
/// call. Rejects bare idents with a span-pointed error (they are ambiguous
/// and would also fail same-crate due to `#[macro_export]` macros resolving
/// only through the crate-root namespace).
///
/// The rewrite collapses nested module segments because `#[macro_export]`
/// publishes the companion macro at the defining crate's root regardless
/// of the module the `ankyra_provider!` invocation lives in. When the
/// first segment is `crate`, the `crate::` prefix is dropped entirely and
/// the returned path is a bare ident — this sidesteps rust-lang/rust#52234
/// ("macro-expanded `macro_export` macros from the current crate cannot be
/// referred to by absolute paths"), which fires when `ankyra_config!` and
/// `ankyra_provider!` live in the same crate. Same-crate resolution still
/// works via the bare name because `#[macro_export]` hoists the macro to
/// the crate root. For external crates the first segment is kept and the
/// intermediate segments are dropped.
pub fn provider_path_to_companion(path: &Path) -> Result<Path, Error> {
    if path.segments.len() < 2 {
        return Err(Error::new_spanned(
            path,
            "provider entries must include at least one leading path segment \
             (e.g. `crate::P` or `some_crate::P`); bare idents cannot resolve \
             to the companion macro because `#[macro_export]` publishes it at \
             the defining crate's root",
        ));
    }
    let first = path.segments.first().cloned().unwrap();
    let last = path.segments.last().cloned().unwrap();
    let companion = provider_companion_ident(&last.ident);

    let mut out = Path {
        leading_colon: None,
        segments: Punctuated::default(),
    };

    // Same-crate references must be written as the bare ident to avoid
    // rust-lang/rust#52234 (an absolute path to a macro-expanded
    // `#[macro_export]` macro in the same crate is rejected). Cross-crate
    // references retain the first segment so rustc can disambiguate the
    // extern crate.
    if first.ident != "crate" {
        out.leading_colon = path.leading_colon;
        out.segments.push(first);
    }
    out.segments.push(syn::PathSegment {
        ident: companion,
        arguments: syn::PathArguments::None,
    });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    fn render(p: &Path) -> String {
        quote::quote!(#p).to_string().replace(' ', "")
    }

    #[test]
    fn collapses_nested_modules_to_crate_root() {
        let input: Path = parse_quote!(clock_lib::nested::deeper::CLOCK_PROVIDER);
        assert_eq!(
            render(&provider_path_to_companion(&input).unwrap()),
            "clock_lib::__ankyra_provider_CLOCK_PROVIDER"
        );
    }

    #[test]
    fn collapses_crate_self_reference_to_bare_ident() {
        // `crate::P` must rewrite to a bare `__ankyra_provider_P` ident so
        // that same-crate uses do not trip rust-lang/rust#52234.
        let input: Path = parse_quote!(crate::CORE_PROVIDER);
        assert_eq!(
            render(&provider_path_to_companion(&input).unwrap()),
            "__ankyra_provider_CORE_PROVIDER"
        );
    }

    #[test]
    fn collapses_nested_crate_reference_to_bare_ident() {
        let input: Path = parse_quote!(crate::nested::deeper::CORE_PROVIDER);
        assert_eq!(
            render(&provider_path_to_companion(&input).unwrap()),
            "__ankyra_provider_CORE_PROVIDER"
        );
    }

    #[test]
    fn rejects_bare_ident() {
        let input: Path = parse_quote!(P);
        assert!(provider_path_to_companion(&input).is_err());
    }
}

#[cfg(test)]
mod pascal_to_snake_tests {
    use super::pascal_to_snake;

    #[test]
    fn simple_pascal_to_snake() {
        assert_eq!(pascal_to_snake("TrsyncState"), "trsync_state");
    }

    #[test]
    fn acronym_run_before_word_collapses() {
        // `ADCValue` → `adc_value` — run of uppercase lowercases together,
        // underscore inserted before the first lowercase-following letter.
        assert_eq!(pascal_to_snake("ADCValue"), "adc_value");
        assert_eq!(pascal_to_snake("SPITransfer"), "spi_transfer");
    }

    #[test]
    fn digits_preserved_no_split() {
        // Digits are not capital boundaries: `HTTPStatus2xx` → `http_status2xx`.
        assert_eq!(pascal_to_snake("HTTPStatus2xx"), "http_status2xx");
    }

    #[test]
    fn already_lowercase_returns_verbatim() {
        // Backward compat: lowercase idents are returned untouched so
        // existing `pub struct trsync_state` consumers keep their wire
        // name.
        assert_eq!(pascal_to_snake("stats"), "stats");
        assert_eq!(pascal_to_snake("trsync_state"), "trsync_state");
        assert_eq!(
            pascal_to_snake("already_with_underscores"),
            "already_with_underscores"
        );
    }

    #[test]
    fn single_char_uppercase_preserved() {
        // A single uppercase letter matches the SCREAMING_SNAKE_CASE
        // passthrough rule (all characters are uppercase / underscore /
        // digit) and is returned verbatim. This is consistent with
        // `MCU` → `MCU` and required so short all-uppercase Klipper
        // constants keep their wire name.
        assert_eq!(pascal_to_snake("A"), "A");
    }

    #[test]
    fn empty_string_is_empty() {
        assert_eq!(pascal_to_snake(""), "");
    }

    #[test]
    fn camel_case_also_converts() {
        // The any-uppercase rule also catches camelCase idents — ankyra
        // treats them as PascalCase for wire-name purposes.
        assert_eq!(pascal_to_snake("myFoo"), "my_foo");
    }

    #[test]
    fn digit_to_upper_inserts_underscore() {
        // `Status2Xxx` → `status2_xxx`: digit→upper is a word boundary in
        // the hand-rolled algorithm (prev is digit, insert underscore).
        assert_eq!(pascal_to_snake("Status2Xxx"), "status2_xxx");
    }

    #[test]
    fn screaming_snake_case_preserved_verbatim() {
        // `#[klipper_constant]` idents follow the Klipper/anchor
        // convention of SCREAMING_SNAKE_CASE, and Klipper's host looks
        // them up verbatim (`get_constant_float("CLOCK_FREQ")`).
        // Lowercasing them breaks the connect handshake.
        assert_eq!(pascal_to_snake("CLOCK_FREQ"), "CLOCK_FREQ");
        assert_eq!(pascal_to_snake("RESERVE_PINS_USB"), "RESERVE_PINS_USB");
        assert_eq!(pascal_to_snake("STATS_SUMSQ_BASE"), "STATS_SUMSQ_BASE");
    }

    #[test]
    fn single_word_all_uppercase_preserved() {
        // `MCU` is all-uppercase but short — it must still be preserved
        // verbatim rather than lowercased to `mcu`.
        assert_eq!(pascal_to_snake("MCU"), "MCU");
    }

    #[test]
    fn screaming_snake_with_digits_preserved() {
        // Digits inside SCREAMING_SNAKE_CASE idents must not trigger
        // the PascalCase conversion path.
        assert_eq!(pascal_to_snake("DATA_32BIT"), "DATA_32BIT");
    }
}

#[cfg(test)]
mod fnv_tests {
    use super::fnv1a_64;

    /// Fixed-value fixture. The matching assertion in
    /// ankyra-assemble/src/shared.rs pins the other side.
    /// Compute `fnv1a_64(b"probe")` once during implementation and pin the
    /// result; do not copy an illustrative value blindly.
    #[test]
    fn fnv1a_64_probe_matches_fixed_value() {
        assert_eq!(fnv1a_64(b"probe"), 0xf976_9124_6db2_66f1);
    }

    #[test]
    fn fnv1a_64_of_empty_is_offset_basis() {
        assert_eq!(fnv1a_64(b""), 0xcbf2_9ce4_8422_2325);
    }
}
