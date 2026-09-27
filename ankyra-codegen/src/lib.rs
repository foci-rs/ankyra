//! Naming and hashing rules shared by ankyra's proc-macro crates.
//!
//! A proc-macro crate can export only macros, so `ankyra-macros` and
//! `ankyra-assemble` take these from this ordinary library. Both sides must
//! derive identical wire names and static-string hashes.

/// Derives the Klipper wire name from a Rust item identifier.
///
/// `PascalCase` and `camelCase` become `snake_case`; identifiers that are
/// already lowercase or `SCREAMING_SNAKE_CASE` are returned unchanged.
#[must_use]
pub fn pascal_to_snake(ident: &str) -> String {
    if !ident.chars().any(char::is_uppercase) {
        return ident.to_string();
    }
    // SCREAMING_SNAKE_CASE passes through: Klipper's host looks constants
    // up verbatim, e.g. `get_constant_float("CLOCK_FREQ")`.
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
        if !c.is_uppercase() {
            out.push(c);
            continue;
        }
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
    }
    out
}

/// FNV-1a 64-bit hash of `bytes`.
///
/// Used instead of `DefaultHasher`, which is not stable across rustc
/// versions, so `__ANKYRA_SS_<hash>` symbol names stay reproducible.
#[must_use]
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

#[cfg(test)]
mod pascal_to_snake_tests {
    use super::pascal_to_snake;

    #[test]
    fn simple_pascal_to_snake() {
        assert_eq!(pascal_to_snake("TrsyncState"), "trsync_state");
    }

    #[test]
    fn acronym_run_before_word_collapses() {
        assert_eq!(pascal_to_snake("ADCValue"), "adc_value");
        assert_eq!(pascal_to_snake("SPITransfer"), "spi_transfer");
    }

    #[test]
    fn digits_preserved_no_split() {
        assert_eq!(pascal_to_snake("HTTPStatus2xx"), "http_status2xx");
    }

    #[test]
    fn already_lowercase_returns_verbatim() {
        assert_eq!(pascal_to_snake("stats"), "stats");
        assert_eq!(pascal_to_snake("trsync_state"), "trsync_state");
        assert_eq!(
            pascal_to_snake("already_with_underscores"),
            "already_with_underscores"
        );
    }

    #[test]
    fn single_char_uppercase_preserved() {
        assert_eq!(pascal_to_snake("A"), "A");
    }

    #[test]
    fn empty_string_is_empty() {
        assert_eq!(pascal_to_snake(""), "");
    }

    #[test]
    fn camel_case_also_converts() {
        assert_eq!(pascal_to_snake("myFoo"), "my_foo");
    }

    #[test]
    fn digit_to_upper_inserts_underscore() {
        assert_eq!(pascal_to_snake("Status2Xxx"), "status2_xxx");
    }

    #[test]
    fn screaming_snake_case_preserved_verbatim() {
        assert_eq!(pascal_to_snake("CLOCK_FREQ"), "CLOCK_FREQ");
        assert_eq!(pascal_to_snake("RESERVE_PINS_USB"), "RESERVE_PINS_USB");
        assert_eq!(pascal_to_snake("STATS_SUMSQ_BASE"), "STATS_SUMSQ_BASE");
    }

    #[test]
    fn single_word_all_uppercase_preserved() {
        assert_eq!(pascal_to_snake("MCU"), "MCU");
    }

    #[test]
    fn screaming_snake_with_digits_preserved() {
        assert_eq!(pascal_to_snake("DATA_32BIT"), "DATA_32BIT");
    }
}

#[cfg(test)]
mod fnv_tests {
    use super::fnv1a_64;

    #[test]
    fn fnv1a_64_probe_matches_fixed_value() {
        assert_eq!(fnv1a_64(b"probe"), 0xf976_9124_6db2_66f1);
    }

    #[test]
    fn fnv1a_64_of_empty_is_offset_basis() {
        assert_eq!(fnv1a_64(b""), 0xcbf2_9ce4_8422_2325);
    }
}
