use proc_macro2::{Ident, Span};
use quote::format_ident;
use syn::punctuated::Punctuated;
use syn::{Error, Path};

pub use ankyra_codegen::{fnv1a_64, item_wire_name};

pub fn descriptor_ident(name: &Ident) -> Ident {
    format_ident!("__ankyra_descriptor_{}", name)
}

pub fn dispatch_ident(name: &Ident) -> Ident {
    format_ident!("__ankyra_dispatch_{}", name)
}

pub fn carrier_ident(kind: &str, name: &Ident) -> Ident {
    format_ident!("__ankyra_item_{}_{}", kind, name)
}

/// The assembler detects the `_lt<N>_` infix to synthesize the
/// `impl<'a0, 'a1, ...>` header of the `SendReply` / `SendOutput` impl.
/// The ident is the only channel that reaches the proc-macro at expansion
/// time: `pub const` values are resolved later by rustc, and invoking a
/// sibling carrier arm in the assembler's crate trips rust-lang/rust#52234.
pub fn carrier_ident_with_lifetimes(kind: &str, name: &Ident, lifetime_count: usize) -> Ident {
    if lifetime_count == 0 {
        carrier_ident(kind, name)
    } else {
        format_ident!("__ankyra_item_{}_lt{}_{}", kind, lifetime_count, name)
    }
}

/// A `pub const` (rather than a carrier-macro arm) can be referenced via
/// `crate::…` paths without tripping rust-lang/rust#52234, which rejects absolute paths to
/// `#[macro_export]` macros from the same crate.
pub fn format_const_ident(kind: &str, name: &Ident) -> Ident {
    format_ident!("__ANKYRA_FORMAT_{}_{}", kind, name)
}

pub fn value_const_ident(kind: &str, name: &Ident) -> Ident {
    format_ident!("__ANKYRA_VALUE_{}_{}", kind, name)
}

pub fn name_const_ident(kind: &str, name: &Ident) -> Ident {
    format_ident!("__ANKYRA_NAME_{}_{}", kind, name)
}

pub fn provider_companion_ident(name: &Ident) -> Ident {
    format_ident!("__ankyra_provider_{}", name)
}

pub fn static_string_hash_ident(msg: &str, span: Span) -> Ident {
    let hash = fnv1a_64(msg.as_bytes());
    format_ident!("__ANKYRA_SS_{:016x}", hash, span = span)
}

/// Intermediate module segments are dropped because `#[macro_export]`
/// publishes the companion macro at the defining crate's root. A leading
/// `crate` segment is dropped too, leaving a bare ident: an absolute path to
/// a macro-expanded `#[macro_export]` macro in the same crate trips
/// rust-lang/rust#52234. For external crates the first segment is kept.
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

fn worst_case_field_width(spec: &str) -> Option<usize> {
    match spec {
        "%c" => Some(2),
        "%hu" | "%hi" => Some(3),
        "%u" | "%i" => Some(5),
        "%*s" | "%.*s" => None,
        other => unreachable!("format spec `{other}` has no wire width"),
    }
}

pub fn wire_size_impl<'a>(
    item: &syn::ItemStruct,
    specs: impl IntoIterator<Item = &'a str>,
) -> proc_macro2::TokenStream {
    let total: Option<usize> = specs.into_iter().map(worst_case_field_width).sum();
    let value = if let Some(n) = total {
        let n = proc_macro2::Literal::usize_unsuffixed(n);
        quote::quote!(::core::option::Option::Some(#n))
    } else {
        quote::quote!(::core::option::Option::None)
    };
    let name = &item.ident;
    let (impl_generics, ty_generics, where_clause) = item.generics.split_for_impl();
    quote::quote! {
        impl #impl_generics ::ankyra::ReplyWireSize for #name #ty_generics #where_clause {
            const MAX_PAYLOAD_BYTES: ::core::option::Option<usize> = #value;
        }
    }
}

#[cfg(test)]
pub fn max_payload_expr(rendered: &str) -> Option<&str> {
    let marker = "const MAX_PAYLOAD_BYTES : :: core :: option :: Option < usize > = ";
    let start = rendered.find(marker)? + marker.len();
    let rest = &rendered[start..];
    let value = &rest[..rest.find(" ;")?];
    Some(
        value
            .strip_prefix(":: core :: option :: Option :: ")
            .unwrap_or(value),
    )
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
