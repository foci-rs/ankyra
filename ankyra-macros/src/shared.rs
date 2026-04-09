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

/// Ident for the hidden re-export emitted by [`crate_root_sniff`] alongside
/// the descriptor fn. The name is kind-qualified so it never collides with
/// a legitimate user ident even if the user writes something unusual.
pub fn crate_root_sniff_ident(kind: &str, name: &Ident) -> Ident {
    format_ident!("__ankyra_root_check_{}_{}", kind, name)
}

/// Emit a compile-time check that fails when a `#[klipper_*]` / `klipper_*!`
/// macro is invoked outside its defining crate's root module.
///
/// The sniff works by emitting a `pub use` at the macro-expansion scope that
/// re-exports `crate::<descriptor_fn>`. When the macro expands at crate
/// root, both the descriptor fn and the `pub use` live at the crate root, so
/// `crate::<descriptor_fn>` resolves to the sibling item. When the macro
/// expands inside a submodule, the descriptor fn lives at
/// `crate::<module>::<descriptor_fn>` — the `crate::<descriptor_fn>` path
/// fails with a clean `E0432 unresolved import` naming the exact descriptor
/// the user should move.
///
/// This encodes the v0.1 invariant that `#[klipper_command]`,
/// `#[klipper_reply]`, `#[klipper_output]`, `#[klipper_constant]`, and
/// `klipper_enumeration!` must be invoked at the defining crate's root
/// (`src/lib.rs` or `src/main.rs`). Lifting this invariant requires a
/// carrier redesign that carries full module paths through the Task 10
/// assembler and is scheduled for v0.2.
pub fn crate_root_sniff(kind: &str, name: &Ident) -> proc_macro2::TokenStream {
    let descriptor = descriptor_ident(name);
    let sniff = crate_root_sniff_ident(kind, name);
    quote::quote! {
        // When the expansion scope is the crate root the `crate::<desc>`
        // path resolves to the sibling descriptor fn and this is a harmless
        // redundant re-export. When the expansion scope is a submodule the
        // descriptor lives at `crate::<mod>::<desc>` and the `pub use` fails
        // with `E0432 unresolved import crate::<desc>`, which points the
        // user straight at the invariant violation.
        #[doc(hidden)]
        #[allow(unused_imports, non_snake_case)]
        pub use crate::#descriptor as #sniff;
    }
}

/// Command-specific variant of [`crate_root_sniff`]: the `#[klipper_command]`
/// macro emits a `__ankyra_dispatch_<name>` fn (not a descriptor fn), so the
/// sniff targets the dispatch fn instead. The diagnostic is analogous — a
/// submodule invocation produces `E0432 unresolved import` naming the
/// dispatch fn the user should move.
pub fn crate_root_sniff_dispatch(name: &Ident) -> proc_macro2::TokenStream {
    let dispatch = dispatch_ident(name);
    let sniff = crate_root_sniff_ident("command", name);
    quote::quote! {
        #[doc(hidden)]
        #[allow(unused_imports, non_snake_case)]
        pub use crate::#dispatch as #sniff;
    }
}

pub fn dispatch_ident(name: &Ident) -> Ident {
    format_ident!("__ankyra_dispatch_{}", name)
}

pub fn carrier_ident(kind: &str, name: &Ident) -> Ident {
    format_ident!("__ankyra_item_{}_{}", kind, name)
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
