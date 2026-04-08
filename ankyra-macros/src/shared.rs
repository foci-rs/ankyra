// Helpers here are consumed by sibling macro modules added in later tasks.
// Allow dead_code until those modules land so that incremental task commits
// compile cleanly under the workspace's `-D warnings` gate.
#![allow(dead_code)]

use proc_macro2::{Ident, Span};
use quote::format_ident;
use syn::punctuated::Punctuated;
use syn::{Error, Path};

pub fn descriptor_ident(name: &Ident) -> Ident {
    format_ident!("__ankyra_descriptor_{}", name)
}

pub fn dispatch_ident(name: &Ident) -> Ident {
    format_ident!("__ankyra_dispatch_{}", name)
}

pub fn carrier_ident(kind: &str, name: &Ident) -> Ident {
    format_ident!("__ankyra_item_{}_{}", kind, name)
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

/// Rewrite `a::b::c::P` into `a::__ankyra_provider_P`. Rejects bare idents
/// with a span-pointed error. First segment is kept (extern-crate name or
/// `crate`); all intermediate segments are dropped because `#[macro_export]`
/// publishes companion macros at the defining crate's root regardless of
/// the module the `ankyra_provider!` invocation lives in.
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
        leading_colon: path.leading_colon,
        segments: Punctuated::default(),
    };
    out.segments.push(first);
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
    fn keeps_crate_self_reference() {
        let input: Path = parse_quote!(crate::CORE_PROVIDER);
        assert_eq!(
            render(&provider_path_to_companion(&input).unwrap()),
            "crate::__ankyra_provider_CORE_PROVIDER"
        );
    }

    #[test]
    fn rejects_bare_ident() {
        let input: Path = parse_quote!(P);
        assert!(provider_path_to_companion(&input).is_err());
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
