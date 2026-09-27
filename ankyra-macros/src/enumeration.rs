//! `klipper_enumeration!` function-like proc-macro.
//!
//! The macro accepts
//!
//! ```ignore
//! klipper_enumeration! {
//!     #[derive(Copy, Clone, Debug, Eq, PartialEq)]
//!     pub enum MotorKind(name = "motor_kind", rename_all = "snake_case") {
//!         BldcMotor,
//!         Stepper,
//!         #[klipper_enumeration(rename = "custom-name")]
//!         Special,
//!         Range(coil, 0, 8),
//!     }
//! }
//! ```
//!
//! and emits:
//!
//! 1. The enum declaration with all plain variants and all expanded
//!    `Range(Prefix, start, count)` pseudo-variant entries (`coil0..coil7`).
//!    The pseudo-variant itself is absorbed into descriptor metadata.
//! 2. `impl From<<Enum>> for <uint>`, where `uint` is the smallest of
//!    `u8`/`u16`/`u32` that holds the maximum variant id.
//! 3. `impl TryFrom<<uint>> for <Enum>` returning
//!    `::ankyra::encoding::ReadError` on out-of-range values.
//! 4. `pub const fn __ankyra_descriptor_<Enum>() -> DefinitionDescriptor` —
//!    the value field is the dictionary JSON object mapping each name to its
//!    id, with the configured `rename_all` applied (and any per-variant
//!    `#[klipper_enumeration(rename = "...")]` overriding the derived name).
//!    A range renders as `"<prefix><start>":[<first_id>,<count>]`; Klipper's
//!    host reads the first variant's number from the key's trailing digits.
//! 5. `#[macro_export] macro_rules! __ankyra_item_enumeration_<Enum>!` —
//!    carrier macro. Tuple shape:
//!    `(enumeration, exported_name, value_json, descriptor_fn_path)`.
//!
//! # `rename_all`
//!
//! Supports the same keywords as serde's `rename_all`: `lowercase`,
//! `UPPERCASE`, `snake_case`, `SCREAMING_SNAKE_CASE`, `camelCase`,
//! `PascalCase`, `kebab-case`. If not specified the variant idents are used
//! verbatim.
//!
//! # Integer width
//!
//! Only the minimum-width unsigned type gets conversions. The command frame
//! parser decides the width at deserialise time, and extra wider impls would
//! invite reliance on a type the wire format does not carry.

use std::collections::HashSet;
use std::str::FromStr;

use ankyra_codegen::{json_escape, snake_case};
use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    Attribute, Error, Ident, LitInt, LitStr, Meta, Token, Visibility, braced, parenthesized,
    parse_macro_input,
};

use crate::shared::{
    carrier_ident, descriptor_ident, name_const_ident, pascal_to_snake, value_const_ident,
};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
enum RenameAll {
    #[default]
    None,
    LowerCase,
    UpperCase,
    SnakeCase,
    ScreamingSnakeCase,
    CamelCase,
    PascalCase,
    KebabCase,
}

impl RenameAll {
    fn apply(self, s: &str) -> String {
        match self {
            Self::None | Self::PascalCase => s.to_owned(),
            Self::LowerCase => s.to_lowercase(),
            Self::UpperCase => s.to_uppercase(),
            Self::SnakeCase => snake_case(s),
            Self::ScreamingSnakeCase => snake_case(s).to_uppercase(),
            Self::CamelCase => upper_camel_to_camel(s),
            Self::KebabCase => snake_case(s).replace('_', "-"),
        }
    }
}

impl FromStr for RenameAll {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "lowercase" => Ok(Self::LowerCase),
            "UPPERCASE" => Ok(Self::UpperCase),
            "snake_case" => Ok(Self::SnakeCase),
            "SCREAMING_SNAKE_CASE" => Ok(Self::ScreamingSnakeCase),
            "camelCase" => Ok(Self::CamelCase),
            "PascalCase" => Ok(Self::PascalCase),
            "kebab-case" => Ok(Self::KebabCase),
            _ => Err(()),
        }
    }
}

fn upper_camel_to_camel(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => {
            let mut out = String::with_capacity(s.len());
            for c in first.to_lowercase() {
                out.push(c);
            }
            out.push_str(chars.as_str());
            out
        }
        None => String::new(),
    }
}

#[derive(Debug, Default)]
struct EnumOptions {
    name: Option<String>,
    rename_all: RenameAll,
}

#[derive(Debug, Default)]
struct VariantOptions {
    rename: Option<String>,
}

impl VariantOptions {
    fn wire_name(&self, ident: &Ident, rename_all: RenameAll) -> String {
        self.rename
            .clone()
            .unwrap_or_else(|| rename_all.apply(&ident.to_string()))
    }
}

fn parse_enum_options(input: ParseStream) -> syn::Result<EnumOptions> {
    let mut opts = EnumOptions::default();
    let punct: Punctuated<Meta, Token![,]> = input.parse_terminated(Meta::parse, Token![,])?;
    for meta in punct {
        match meta {
            Meta::NameValue(m) if m.path.is_ident("name") => {
                opts.name = Some(expr_to_lit_str(&m.value)?.value());
            }
            Meta::NameValue(m) if m.path.is_ident("rename_all") => {
                let lit = expr_to_lit_str(&m.value)?;
                let fmt = lit.value();
                opts.rename_all = fmt.parse().map_err(|()| {
                    Error::new(
                        lit.span(),
                        "unknown rename_all value; expected one of: \
                         lowercase, UPPERCASE, snake_case, SCREAMING_SNAKE_CASE, \
                         camelCase, PascalCase, kebab-case",
                    )
                })?;
            }
            other => {
                return Err(Error::new(
                    other.span(),
                    "unknown klipper_enumeration header option; expected `name = \"...\"` or `rename_all = \"...\"`",
                ));
            }
        }
    }
    Ok(opts)
}

fn expr_to_lit_str(expr: &syn::Expr) -> syn::Result<&LitStr> {
    if let syn::Expr::Lit(syn::ExprLit {
        lit: syn::Lit::Str(s),
        ..
    }) = expr
    {
        Ok(s)
    } else {
        Err(Error::new(expr.span(), "expected string literal"))
    }
}

fn parse_variant_options(attrs: &[Attribute]) -> syn::Result<VariantOptions> {
    let mut opts = VariantOptions::default();
    for attr in attrs
        .iter()
        .filter(|a| a.path().is_ident("klipper_enumeration"))
    {
        let items: Punctuated<Meta, Token![,]> =
            attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
        for m in items {
            match m {
                Meta::NameValue(nv) if nv.path.is_ident("rename") => {
                    opts.rename = Some(expr_to_lit_str(&nv.value)?.value());
                }
                other => {
                    return Err(Error::new(
                        other.span(),
                        "unknown klipper_enumeration variant option; expected `rename = \"...\"`",
                    ));
                }
            }
        }
    }
    Ok(opts)
}

#[derive(Debug)]
enum EnumVariant {
    Single {
        attrs: Vec<Attribute>,
        opts: VariantOptions,
        ident: Ident,
    },
    Range {
        attrs: Vec<Attribute>,
        opts: VariantOptions,
        prefix: Ident,
        start: usize,
        count: usize,
    },
}

impl EnumVariant {
    fn count(&self) -> usize {
        match self {
            Self::Single { .. } => 1,
            Self::Range { count, .. } => *count,
        }
    }
}

impl Parse for EnumVariant {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let attrs = input.call(Attribute::parse_outer)?;
        let (ours, passthrough): (Vec<_>, Vec<_>) = attrs
            .into_iter()
            .partition(|a| a.path().is_ident("klipper_enumeration"));
        let opts = parse_variant_options(&ours)?;

        let _vis: Visibility = input.parse()?;
        let ident: Ident = input.parse()?;

        if ident == "Range" {
            let content;
            let _parens = parenthesized!(content in input);
            let prefix: Ident = content.parse()?;
            content.parse::<Token![,]>()?;
            let start: usize = content.parse::<LitInt>()?.base10_parse()?;
            content.parse::<Token![,]>()?;
            let count: usize = content.parse::<LitInt>()?.base10_parse()?;
            if count == 0 {
                return Err(Error::new(ident.span(), "Range count must be at least 1"));
            }
            Ok(Self::Range {
                attrs: passthrough,
                opts,
                prefix,
                start,
                count,
            })
        } else {
            Ok(Self::Single {
                attrs: passthrough,
                opts,
                ident,
            })
        }
    }
}

struct Enumeration {
    attrs: Vec<Attribute>,
    visibility: Visibility,
    ident: Ident,
    options: EnumOptions,
    variants: Vec<EnumVariant>,
}

impl Parse for Enumeration {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let attrs = input.call(Attribute::parse_outer)?;
        let visibility: Visibility = input.parse()?;
        input.parse::<Token![enum]>()?;
        let ident: Ident = input.parse()?;

        let options = if input.peek(syn::token::Paren) {
            let header;
            let _parens = parenthesized!(header in input);
            parse_enum_options(&header)?
        } else {
            EnumOptions::default()
        };

        let body;
        let _braces = braced!(body in input);
        let variants: Punctuated<EnumVariant, Token![,]> =
            body.parse_terminated(EnumVariant::parse, Token![,])?;
        let variants: Vec<EnumVariant> = variants.into_iter().collect();
        reject_duplicate_wire_names(&variants, options.rename_all)?;

        Ok(Self {
            attrs,
            visibility,
            ident,
            options,
            variants,
        })
    }
}

/// Rejects variants whose dictionary names coincide after `rename_all`,
/// per-variant renames, and range expansion; the host keeps only one of them.
fn reject_duplicate_wire_names(variants: &[EnumVariant], rename_all: RenameAll) -> syn::Result<()> {
    let mut seen = HashSet::new();
    for v in variants {
        let (span, names) = match v {
            EnumVariant::Single { ident, opts, .. } => {
                (ident.span(), vec![opts.wire_name(ident, rename_all)])
            }
            EnumVariant::Range {
                prefix,
                opts,
                start,
                count,
                ..
            } => {
                let base = opts.wire_name(prefix, rename_all);
                let names = (*start..start + count)
                    .map(|n| format!("{base}{n}"))
                    .collect();
                (prefix.span(), names)
            }
        };
        for name in names {
            if !seen.insert(name.clone()) {
                return Err(Error::new(
                    span,
                    format!("enumeration name `{name}` is used by more than one variant"),
                ));
            }
        }
    }
    Ok(())
}

fn width_for_max(max: usize) -> &'static str {
    match max {
        0..=255 => "u8",
        256..=65_535 => "u16",
        _ => "u32",
    }
}

fn numbered(variants: &[EnumVariant]) -> Vec<(&EnumVariant, usize, usize)> {
    let mut out = Vec::with_capacity(variants.len());
    let mut cursor = 0usize;
    for v in variants {
        let c = v.count();
        out.push((v, cursor, c));
        cursor += c;
    }
    out
}

pub fn expand_enumeration(input: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(input as Enumeration);
    expand_enumeration_impl(&parsed).into()
}

fn build_variant_decls(variants: &[EnumVariant]) -> Vec<TokenStream2> {
    variants
        .iter()
        .flat_map(|v| match v {
            EnumVariant::Single { attrs, ident, .. } => vec![quote! {
                #(#attrs)*
                #ident,
            }],
            EnumVariant::Range {
                attrs,
                prefix,
                start,
                count,
                ..
            } => (*start..*start + *count)
                .map(|n| {
                    let name = format_ident!("{}{}", prefix, n);
                    quote! {
                        #(#attrs)*
                        #name,
                    }
                })
                .collect(),
        })
        .collect()
}

fn build_match_arms(
    enum_ident: &Ident,
    numbered_variants: &[(&EnumVariant, usize, usize)],
    id_lit: impl Fn(usize) -> syn::LitInt,
) -> (Vec<TokenStream2>, Vec<TokenStream2>) {
    let mut to_arms = Vec::new();
    let mut from_arms = Vec::new();
    for (v, start, count) in numbered_variants {
        match v {
            EnumVariant::Single { ident, .. } => {
                let id = id_lit(*start);
                to_arms.push(quote! { #enum_ident::#ident => #id, });
                from_arms.push(quote! { #id => ::core::result::Result::Ok(#enum_ident::#ident), });
            }
            EnumVariant::Range {
                prefix,
                start: ident_start,
                ..
            } => {
                for i in 0..*count {
                    let name = format_ident!("{}{}", prefix, ident_start + i);
                    let id = id_lit(*start + i);
                    to_arms.push(quote! { #enum_ident::#name => #id, });
                    from_arms
                        .push(quote! { #id => ::core::result::Result::Ok(#enum_ident::#name), });
                }
            }
        }
    }
    (to_arms, from_arms)
}

/// Shape matches the host-side contract:
///
/// * Plain variants render as `"<name>":<id>`.
/// * `Range(prefix, start, count)` variants collapse to a single
///   `"<prefix><start>":[<first_id>,<count>]` entry — the host expands this into
///   `<prefix><start>..<prefix><start+count-1>` at parse time, matching
///   Klipper's `pin` / `bus` enumeration conventions.
fn build_json_value(
    numbered_variants: &[(&EnumVariant, usize, usize)],
    rename_all: RenameAll,
) -> String {
    let mut out = String::from("{");
    for (idx, (v, start, count)) in numbered_variants.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        match v {
            EnumVariant::Single { ident, opts, .. } => {
                let name = opts.wire_name(ident, rename_all);
                out.push('"');
                out.push_str(&json_escape(&name));
                out.push_str("\":");
                out.push_str(&start.to_string());
            }
            EnumVariant::Range {
                prefix,
                opts,
                start: ident_start,
                ..
            } => {
                let first = format!("{}{ident_start}", opts.wire_name(prefix, rename_all));
                out.push('"');
                out.push_str(&json_escape(&first));
                out.push_str("\":[");
                out.push_str(&start.to_string());
                out.push(',');
                out.push_str(&count.to_string());
                out.push(']');
            }
        }
    }
    out.push('}');
    out
}

fn expand_enumeration_impl(e: &Enumeration) -> TokenStream2 {
    let enum_ident = &e.ident;
    let visibility = &e.visibility;
    let attrs = &e.attrs;

    let variant_decls = build_variant_decls(&e.variants);

    let numbered_variants = numbered(&e.variants);
    let max_id = numbered_variants
        .iter()
        .last()
        .map_or(0, |(_, s, c)| s + c - 1);
    let width = format_ident!("{}", width_for_max(max_id));

    // Suffixed literals, not `N as uT` casts, because the ids are also used
    // as match patterns.
    let id_lit = |n: usize| -> syn::LitInt {
        syn::LitInt::new(&format!("{n}{width}"), proc_macro2::Span::call_site())
    };

    let (to_arms, from_arms) = build_match_arms(enum_ident, &numbered_variants, id_lit);
    let json_value = build_json_value(&numbered_variants, e.options.rename_all);

    let exported_name = e
        .options
        .name
        .clone()
        .unwrap_or_else(|| pascal_to_snake(&enum_ident.to_string()));

    let descriptor_fn_name = descriptor_ident(enum_ident);
    let carrier_name = carrier_ident("enumeration", enum_ident);
    let value_const_name = value_const_ident("enumeration", enum_ident);
    let name_const_name = name_const_ident("enumeration", enum_ident);

    let descriptor_fn = quote! {
        #[doc(hidden)]
        #[allow(non_snake_case)]
        pub const fn #descriptor_fn_name() -> ::ankyra::descriptor::DefinitionDescriptor {
            ::ankyra::descriptor::DefinitionDescriptor::new(
                ::ankyra::descriptor::DefinitionKind::Enumeration,
                #exported_name,
                #json_value,
            )
        }
    };

    let name_const = quote! {
        #[doc(hidden)]
        #[allow(non_upper_case_globals)]
        pub const #name_const_name: &str = #exported_name;
    };
    let value_const = quote! {
        #[doc(hidden)]
        #[allow(non_upper_case_globals)]
        pub const #value_const_name: &str = #json_value;
    };

    let carrier = quote! {
        #[doc(hidden)]
        #[macro_export]
        macro_rules! #carrier_name {
            (kind) => { "enumeration" };
            (name) => { #exported_name };
            (value) => { #json_value };
            (descriptor_path) => { $crate::#descriptor_fn_name };
            () => {
                (enumeration, #exported_name, #json_value, $crate::#descriptor_fn_name)
            };
        }
    };

    quote! {
        #(#attrs)*
        #[allow(non_camel_case_types)]
        #visibility enum #enum_ident {
            #(#variant_decls)*
        }

        impl ::core::convert::From<#enum_ident> for #width {
            fn from(value: #enum_ident) -> #width {
                match value {
                    #(#to_arms)*
                }
            }
        }

        impl ::core::convert::TryFrom<#width> for #enum_ident {
            type Error = ::ankyra::encoding::ReadError;

            fn try_from(value: #width) -> ::core::result::Result<Self, Self::Error> {
                match value {
                    #(#from_arms)*
                    _ => ::core::result::Result::Err(::ankyra::encoding::ReadError),
                }
            }
        }

        #descriptor_fn
        #name_const
        #value_const
        #carrier
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    fn render(ts: &TokenStream2) -> String {
        ts.to_string()
    }

    fn expand_for_test(input: TokenStream2) -> TokenStream2 {
        let parsed: Enumeration = syn::parse2(input).expect("parse Enumeration");
        expand_enumeration_impl(&parsed)
    }

    #[test]
    fn camel_case_lowercases_first_char_only() {
        assert_eq!(upper_camel_to_camel("MotorKind"), "motorKind");
        assert_eq!(upper_camel_to_camel("X"), "x");
    }

    #[test]
    fn width_picked_from_max_variant_id() {
        assert_eq!(width_for_max(0), "u8");
        assert_eq!(width_for_max(255), "u8");
        assert_eq!(width_for_max(256), "u16");
        assert_eq!(width_for_max(65_535), "u16");
        assert_eq!(width_for_max(65_536), "u32");
    }

    #[test]
    fn emits_expected_core_items_for_simple_enum() {
        let input = quote! {
            pub enum MotorKind(name = "motor_kind", rename_all = "snake_case") {
                BldcMotor,
                Stepper,
            }
        };
        let out = render(&expand_for_test(input));
        assert!(out.contains("enum MotorKind"), "enum decl missing: {out}");
        assert!(
            out.contains("From < MotorKind > for u8"),
            "From<Enum> missing: {out}"
        );
        assert!(
            out.contains("TryFrom < u8 > for MotorKind"),
            "TryFrom missing: {out}"
        );
        assert!(
            out.contains("__ankyra_descriptor_MotorKind"),
            "descriptor fn missing: {out}"
        );
        assert!(
            out.contains(r#"Enumeration , "motor_kind" , "{\"bldc_motor\":0,\"stepper\":1}""#),
            "descriptor value is not the dictionary JSON: {out}"
        );
        assert!(
            out.contains("__ankyra_item_enumeration_MotorKind"),
            "carrier missing: {out}"
        );
    }

    #[test]
    fn range_variants_expand_and_name_entries() {
        let input = quote! {
            pub enum Pin(rename_all = "snake_case") {
                Led,
                Range(coil, 0, 3),
            }
        };
        let out = render(&expand_for_test(input));
        assert!(out.contains("coil0 ,"), "coil0 missing: {out}");
        assert!(out.contains("coil1 ,"), "coil1 missing: {out}");
        assert!(out.contains("coil2 ,"), "coil2 missing: {out}");
        assert!(
            out.contains(r#""{\"led\":0,\"coil0\":[1,3]}""#),
            "range entry wrong: {out}"
        );
    }

    #[test]
    fn range_key_carries_the_start_index() {
        let input = quote! {
            pub enum Pin {
                Led,
                Range(coil, 3, 4),
            }
        };
        let out = render(&expand_for_test(input));
        assert!(out.contains("coil3 ,"), "coil3 missing: {out}");
        assert!(
            out.contains(r#""{\"Led\":0,\"coil3\":[1,4]}""#),
            "range key must name the first variant: {out}"
        );
    }

    #[test]
    fn range_key_keeps_a_prefix_ending_in_a_digit() {
        let input = quote! {
            pub enum Chan {
                Range(ch1, 0, 2),
            }
        };
        let out = render(&expand_for_test(input));
        assert!(out.contains("ch10 ,"), "ch10 missing: {out}");
        assert!(
            out.contains(r#""{\"ch10\":[0,2]}""#),
            "range key must name the first variant: {out}"
        );
    }

    #[test]
    fn rename_all_snake_case_splits_words_like_wire_names() {
        let input = quote! {
            pub enum Link(rename_all = "snake_case") {
                V2X,
                Foo_Bar,
                MCU,
            }
        };
        let out = render(&expand_for_test(input));
        assert!(
            out.contains(r#""{\"v2_x\":0,\"foo_bar\":1,\"mcu\":2}""#),
            "rename_all snake_case wrong: {out}"
        );
    }

    fn parse_error(input: TokenStream2) -> String {
        match syn::parse2::<Enumeration>(input) {
            Ok(_) => panic!("expected a parse error"),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn rejects_variants_that_normalize_to_one_name() {
        let err = parse_error(quote! {
            pub enum Link(rename_all = "snake_case") {
                FooBar,
                Foo_Bar,
            }
        });
        assert!(err.contains("foo_bar"), "{err}");
    }

    #[test]
    fn rejects_a_rename_that_matches_a_range_member() {
        let err = parse_error(quote! {
            pub enum Pin {
                #[klipper_enumeration(rename = "coil1")]
                Led,
                Range(coil, 0, 3),
            }
        });
        assert!(err.contains("coil1"), "{err}");
    }

    #[test]
    fn width_grows_with_variant_count() {
        let input = quote! {
            pub enum Big {
                Range(n, 0, 300),
            }
        };
        let out = render(&expand_for_test(input));
        assert!(
            out.contains("From < Big > for u16"),
            "expected u16 width: {out}"
        );
    }

    #[test]
    fn per_variant_rename_overrides_rename_all() {
        let input = quote! {
            pub enum M(rename_all = "snake_case") {
                #[klipper_enumeration(rename = "custom-name")]
                Special,
            }
        };
        let out = render(&expand_for_test(input));
        assert!(
            out.contains(r#""{\"custom-name\":0}""#),
            "per-variant rename not applied: {out}"
        );
    }
}
