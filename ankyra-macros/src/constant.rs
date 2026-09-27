//! `#[klipper_constant]` attribute expansion.
//!
//! Applied to a plain `pub const` of the form
//!
//! ```ignore
//! #[klipper_constant]
//! pub const CLOCK_FREQ: u32 = 168_000_000;
//! ```
//!
//! or
//!
//! ```ignore
//! #[klipper_constant]
//! pub const MCU: &str = "stm32f407";
//! ```
//!
//! and emits three sibling items:
//!
//! 1. The original `pub const` passthrough — user-controlled attributes are
//!    preserved; no extra attributes are forced.
//! 2. `pub const fn __ankyra_descriptor_<NAME>() -> DefinitionDescriptor` —
//!    the descriptor the assembler consumes. The descriptor's value
//!    field is the stringified form of the const expression (integer literal
//!    decimal form; string literal verbatim content) so that the assembler
//!    can inject it into the Klipper data dictionary without re-evaluating
//!    the const.
//! 3. `#[macro_export] macro_rules! __ankyra_item_constant_<NAME>!` — the
//!    carrier macro. Tuple shape mirrors the reply/output carriers:
//!    `(constant, exported_name, stringified_value, descriptor_fn_path)`.
//!
//! # Type allowlist
//!
//! Only `u32` and `&str` are accepted. Other integer widths and primitive
//! types are rejected with a span-pointed diagnostic on the type because the
//! Klipper data dictionary wire protocol distinguishes only these two forms
//! for constants — widening a `u16` to `u32` is the intended fix. Floats,
//! booleans, arrays, and user-defined types are similarly out of scope.

use proc_macro::TokenStream;
use proc_macro_error2::abort;
use proc_macro2::TokenStream as TokenStream2;
use quote::{ToTokens, quote};
use syn::spanned::Spanned;
use syn::{Expr, ExprLit, ItemConst, Lit, Type, TypePath, TypeReference, parse_macro_input};

use crate::shared::{
    carrier_ident, descriptor_ident, name_const_ident, pascal_to_snake, value_const_ident,
};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum ConstType {
    U32,
    Str,
}

fn classify_type(ty: &Type) -> Option<ConstType> {
    match ty {
        Type::Path(tp) => classify_path_type(tp),
        Type::Reference(tr) => classify_reference_type(tr),
        _ => None,
    }
}

fn classify_path_type(tp: &TypePath) -> Option<ConstType> {
    if tp.qself.is_some() {
        return None;
    }
    let ident = tp.path.get_ident()?.to_string();
    match ident.as_str() {
        "u32" => Some(ConstType::U32),
        _ => None,
    }
}

fn classify_reference_type(tr: &TypeReference) -> Option<ConstType> {
    if tr.mutability.is_some() {
        return None;
    }
    match tr.elem.as_ref() {
        Type::Path(tp) if tp.path.is_ident("str") => Some(ConstType::Str),
        _ => None,
    }
}

pub(crate) fn json_escape(s: &str) -> String {
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

/// Non-literal const expressions (e.g. `0 + 1`) are rejected: the assembler
/// needs the literal text at macro-expansion time, and const-evaluating
/// arbitrary expressions at proc-macro time is not possible.
fn stringify_expr(kind: ConstType, expr: &Expr) -> Result<String, proc_macro2::Span> {
    let lit = match expr {
        Expr::Lit(ExprLit { lit, .. }) => lit,
        Expr::Unary(syn::ExprUnary {
            op: syn::UnOp::Neg(_),
            expr: inner,
            ..
        }) => return Err(inner.span()),
        other => return Err(other.span()),
    };

    match (kind, lit) {
        (ConstType::U32, Lit::Int(int_lit)) => match int_lit.base10_parse::<u64>() {
            Ok(n) => Ok(n.to_string()),
            Err(_) => Err(int_lit.span()),
        },
        (ConstType::Str, Lit::Str(str_lit)) => Ok(str_lit.value()),
        (_, _) => Err(lit.span()),
    }
}

pub fn expand_constant(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_const = parse_macro_input!(item as ItemConst);
    expand_constant_impl(&item_const).into()
}

fn expand_constant_impl(item: &ItemConst) -> TokenStream2 {
    let name = &item.ident;
    let ty = item.ty.as_ref();

    let Some(kind) = classify_type(ty) else {
        let rendered = ty.to_token_stream().to_string();
        abort!(
            ty,
            "#[klipper_constant] has unsupported type `{}`. Supported types: u32, &str.",
            rendered
        );
    };

    let value_string = match stringify_expr(kind, item.expr.as_ref()) {
        Ok(s) => s,
        Err(span) => abort!(
            span,
            "#[klipper_constant] requires a plain literal initialiser \
             (integer literal for `u32`, string literal for `&str`); \
             non-literal expressions are not supported."
        ),
    };

    let json_value_string = match kind {
        ConstType::U32 => value_string.clone(),
        ConstType::Str => format!("\"{}\"", json_escape(&value_string)),
    };

    let exported_name = pascal_to_snake(&name.to_string());

    let descriptor_fn_name = descriptor_ident(name);
    let carrier_name = carrier_ident("constant", name);
    let value_const_name = value_const_ident("constant", name);
    let name_const_name = name_const_ident("constant", name);

    let descriptor_fn = quote! {
        #[doc(hidden)]
        #[allow(non_snake_case)]
        pub const fn #descriptor_fn_name() -> ::ankyra::descriptor::DefinitionDescriptor {
            ::ankyra::descriptor::DefinitionDescriptor::new(
                ::ankyra::descriptor::DefinitionKind::Constant,
                #exported_name,
                #value_string,
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
        pub const #value_const_name: &str = #json_value_string;
    };

    let carrier = quote! {
        #[doc(hidden)]
        #[macro_export]
        macro_rules! #carrier_name {
            (kind) => { "constant" };
            (name) => { #exported_name };
            (value) => { #json_value_string };
            (descriptor_path) => { $crate::#descriptor_fn_name };
            () => {
                (constant, #exported_name, #value_string, $crate::#descriptor_fn_name)
            };
        }
    };

    quote! {
        #item
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
        let item: ItemConst = syn::parse2(input).expect("parse ItemConst");
        expand_constant_impl(&item)
    }

    #[test]
    fn emits_descriptor_and_carrier_for_u32() {
        let input = quote! {
            pub const CLOCK_FREQ: u32 = 168_000_000;
        };
        let out = render(&expand_for_test(input));
        assert!(
            out.contains("pub const CLOCK_FREQ : u32 = 168_000_000"),
            "original const not preserved: {out}"
        );
        assert!(
            out.contains("__ankyra_descriptor_CLOCK_FREQ"),
            "missing descriptor fn: {out}"
        );
        assert!(
            out.contains("__ankyra_item_constant_CLOCK_FREQ"),
            "missing carrier macro: {out}"
        );
        assert!(
            out.contains("\"CLOCK_FREQ\""),
            "exported_name missing: {out}"
        );
        assert!(
            out.contains("\"168000000\""),
            "value_string missing/wrong: {out}"
        );
        assert!(
            out.contains("DefinitionKind :: Constant"),
            "DefinitionKind::Constant missing: {out}"
        );
    }

    #[test]
    fn emits_descriptor_and_carrier_for_str() {
        let input = quote! {
            pub const MCU: &str = "stm32f407";
        };
        let out = render(&expand_for_test(input));
        assert!(
            out.contains("pub const MCU : & str = \"stm32f407\""),
            "original const not preserved: {out}"
        );
        assert!(out.contains("__ankyra_descriptor_MCU"), "missing fn: {out}");
        assert!(
            out.contains("__ankyra_item_constant_MCU"),
            "missing carrier: {out}"
        );
        assert!(out.contains("\"stm32f407\""), "value_string missing: {out}");
    }

    #[test]
    fn underscores_stripped_from_int_literal() {
        let input = quote! {
            pub const N: u32 = 1_000_000u32;
        };
        let out = render(&expand_for_test(input));
        assert!(out.contains("\"1000000\""), "value not normalised: {out}");
    }
}
