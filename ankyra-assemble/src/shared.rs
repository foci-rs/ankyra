//! Helpers shared within the assembler crate. A verbatim mirror of the
//! subset of `ankyra-macros::shared` the assembler needs, kept local so the
//! assembler does not depend on the other macro crate.

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

/// Derive the Klipper wire name from a Rust item identifier.
///
/// Verbatim mirror of [`ankyra_macros::shared::pascal_to_snake`], kept
/// local so the assembler does not depend on the proc-macro crate.
///
/// If the ident contains any uppercase letter, treat it as `PascalCase` /
/// `camelCase` and convert to `snake_case`. Already-lowercase idents are
/// returned verbatim (backward compat with consumers that hand-rolled
/// `snake_case` struct idents).
///
/// See the macro-side doc comment for the full rule set and examples.
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

#[cfg(test)]
mod pascal_to_snake_parity_tests {
    use super::pascal_to_snake;

    /// The assembler's copy of `pascal_to_snake` must behave identically
    /// to the proc-macro copy. Any divergence would cause the assembler to
    /// dedup under a different wire name than the macro emits — which
    /// surfaces as a mismatched `__ANKYRA_NAME_*` const lookup at the
    /// dictionary emission stage. Re-pin the same fixtures here.
    #[test]
    fn pascal_to_snake_parity_fixtures() {
        assert_eq!(pascal_to_snake("TrsyncState"), "trsync_state");
        assert_eq!(pascal_to_snake("ADCValue"), "adc_value");
        assert_eq!(pascal_to_snake("SPITransfer"), "spi_transfer");
        assert_eq!(pascal_to_snake("HTTPStatus2xx"), "http_status2xx");
        assert_eq!(pascal_to_snake("stats"), "stats");
        assert_eq!(pascal_to_snake("trsync_state"), "trsync_state");
    }

    /// `SCREAMING_SNAKE_CASE` idents (the Klipper/anchor convention
    /// for `#[klipper_constant]` items) must pass through both copies
    /// verbatim. The assembler and macro copies must agree, or the
    /// assembler will look up a constant under a different wire name
    /// than the macro emits.
    #[test]
    fn screaming_snake_case_parity_fixtures() {
        assert_eq!(pascal_to_snake("CLOCK_FREQ"), "CLOCK_FREQ");
        assert_eq!(pascal_to_snake("RESERVE_PINS_USB"), "RESERVE_PINS_USB");
        assert_eq!(pascal_to_snake("STATS_SUMSQ_BASE"), "STATS_SUMSQ_BASE");
        assert_eq!(pascal_to_snake("MCU"), "MCU");
        assert_eq!(pascal_to_snake("DATA_32BIT"), "DATA_32BIT");
    }
}

#[cfg(test)]
mod fnv_tests {
    use super::fnv1a_64;

    /// Fixed-value fixture. The matching assertion in
    /// `ankyra-macros/src/shared.rs` pins the other side; any divergence
    /// between the two copies must fail at `cargo test`.
    #[test]
    fn fnv1a_64_probe_matches_fixed_value() {
        assert_eq!(fnv1a_64(b"probe"), 0xf976_9124_6db2_66f1);
    }

    #[test]
    fn fnv1a_64_of_empty_is_offset_basis() {
        assert_eq!(fnv1a_64(b""), 0xcbf2_9ce4_8422_2325);
    }
}
