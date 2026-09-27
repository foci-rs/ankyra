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
pub fn item_wire_name(ident: &str) -> String {
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
    snake_case(ident)
}

#[doc(hidden)]
pub use item_wire_name as pascal_to_snake;

/// Converts `PascalCase`, `camelCase` or `SCREAMING_SNAKE_CASE` to `snake_case`.
///
/// A word starts at an uppercase letter that follows a lowercase letter or a
/// digit, or that ends an acronym run (`HTTPRequest` becomes `http_request`).
#[must_use]
pub fn snake_case(ident: &str) -> String {
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

/// Escapes `s` for use inside a JSON string literal.
///
/// Quotes, backslashes and control bytes are escaped; everything else,
/// including non-ASCII text, passes through as UTF-8.
#[must_use]
pub fn json_escape(s: &str) -> String {
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
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod item_wire_name_tests {
    use super::item_wire_name;

    #[test]
    fn simple_pascal_to_snake() {
        assert_eq!(item_wire_name("TrsyncState"), "trsync_state");
    }

    #[test]
    fn acronym_run_before_word_collapses() {
        assert_eq!(item_wire_name("ADCValue"), "adc_value");
        assert_eq!(item_wire_name("SPITransfer"), "spi_transfer");
    }

    #[test]
    fn digits_preserved_no_split() {
        assert_eq!(item_wire_name("HTTPStatus2xx"), "http_status2xx");
    }

    #[test]
    fn already_lowercase_returns_verbatim() {
        assert_eq!(item_wire_name("stats"), "stats");
        assert_eq!(item_wire_name("trsync_state"), "trsync_state");
        assert_eq!(
            item_wire_name("already_with_underscores"),
            "already_with_underscores"
        );
    }

    #[test]
    fn single_char_uppercase_preserved() {
        assert_eq!(item_wire_name("A"), "A");
    }

    #[test]
    fn empty_string_is_empty() {
        assert_eq!(item_wire_name(""), "");
    }

    #[test]
    fn camel_case_also_converts() {
        assert_eq!(item_wire_name("myFoo"), "my_foo");
    }

    #[test]
    fn digit_to_upper_inserts_underscore() {
        assert_eq!(item_wire_name("Status2Xxx"), "status2_xxx");
    }

    #[test]
    fn screaming_snake_case_preserved_verbatim() {
        assert_eq!(item_wire_name("CLOCK_FREQ"), "CLOCK_FREQ");
        assert_eq!(item_wire_name("RESERVE_PINS_USB"), "RESERVE_PINS_USB");
        assert_eq!(item_wire_name("STATS_SUMSQ_BASE"), "STATS_SUMSQ_BASE");
    }

    #[test]
    fn single_word_all_uppercase_preserved() {
        assert_eq!(item_wire_name("MCU"), "MCU");
    }

    #[test]
    fn screaming_snake_with_digits_preserved() {
        assert_eq!(item_wire_name("DATA_32BIT"), "DATA_32BIT");
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

#[cfg(test)]
mod snake_case_tests {
    use super::snake_case;

    #[test]
    fn lowercases_all_caps_words() {
        assert_eq!(snake_case("MCU"), "mcu");
        assert_eq!(snake_case("CLOCK_FREQ"), "clock_freq");
    }

    #[test]
    fn splits_acronym_runs_before_words() {
        assert_eq!(snake_case("HTTPRequest"), "http_request");
    }

    #[test]
    fn splits_after_a_digit() {
        assert_eq!(snake_case("V2X"), "v2_x");
    }

    #[test]
    fn keeps_a_single_existing_underscore() {
        assert_eq!(snake_case("Foo_Bar"), "foo_bar");
    }
}

#[cfg(test)]
mod json_escape_tests {
    use super::json_escape;

    #[test]
    fn escapes_quote_and_backslash() {
        assert_eq!(json_escape(r#"a"b\c"#), r#"a\"b\\c"#);
    }

    #[test]
    fn escapes_named_and_other_control_bytes() {
        assert_eq!(json_escape("\n\r\t\u{8}\u{c}"), r"\n\r\t\b\f");
        assert_eq!(json_escape("\u{1}"), r"\u0001");
    }

    #[test]
    fn passes_non_ascii_through() {
        assert_eq!(json_escape("µs"), "µs");
    }
}
