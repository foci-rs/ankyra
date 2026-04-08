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
