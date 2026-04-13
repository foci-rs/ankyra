//! `#[klipper_output]` attribute and `klipper_output!` call-site expansion.
//!
//! # `#[klipper_output]` attribute
//!
//! Applied to a plain struct of the form
//!
//! ```ignore
//! #[klipper_output]
//! pub struct DebugPrint {
//!     pub value: u32,
//!     pub label: i16,
//! }
//! ```
//!
//! or with an explicit Klipper-style printf format:
//!
//! ```ignore
//! #[klipper_output(format = "debug v=%u s=%.*s")]
//! pub struct Hello<'a> {
//!     pub v: u32,
//!     pub s: &'a str,
//! }
//! ```
//!
//! and emits, in order:
//!
//! 1. The original struct passthrough — user-controlled derives and attrs are
//!    preserved. No extra derives are forced.
//! 2. `impl ::ankyra::reply::OutputPayload for <T>` — the marker trait,
//!    forwarded over the struct's generics (including any lifetime carried
//!    by `&[u8]`/`&str` fields).
//! 3. `impl ::ankyra::encoding::Writable for <T>` — iterates fields in
//!    declaration order and delegates to each field type's own `Writable`
//!    impl.
//! 4. `pub const fn __ankyra_descriptor_<T>() -> OutputDescriptor` — returns
//!    a descriptor with the protocol name and the Klipper-style message
//!    format. The format string is either the one the user supplied via
//!    `#[klipper_output(format = "...")]` or — when unspecified —
//!    synthesized at macro-expansion time from the validated field types as
//!    `"<Struct> <field1>=%<spec1> ..."`.
//! 5. `#[macro_export] macro_rules! __ankyra_item_output_<T>!` — carrier
//!    macro consumed by the Task 10 assembler. Tuple shape mirrors the
//!    reply carrier but with kind ident `output`:
//!    `(output, protocol_name, message_format, descriptor_fn_path)`.
//!
//! Field types are validated against the same wire-type allowlist as
//! command arguments and reply fields (primitives, `&[u8]`, `&str`).
//! Unions, enums, and tuple structs are rejected.
//!
//! ## Format-string cross-check
//!
//! When the user supplies `format = "..."`, the format string is parsed
//! (simple left-to-right scanner — not a full printf parser) to extract
//! `%<spec>` tokens in order. Each extracted specifier is then cross-checked
//! against the corresponding struct field's declared type; mismatches abort
//! expansion with a span-pointed error on the *field* (more actionable than
//! the literal) naming both expected and declared types.
//!
//! # `klipper_output!` call-site macro
//!
//! Parsed as `klipper_output!(O, field1 [: ty] = expr, field2 [: ty] = expr, ...)`
//! and emits `<_ as ::ankyra::SendOutput<O>>::send(__ankyra_sender, O { ... })`.
//! As with `klipper_reply!`, this must appear inside a `#[klipper_command]`
//! handler body because the emitted code references `__ankyra_sender`, the
//! injected formal parameter.
//!
//! The optional `: ty` annotation per field is documentary; it is parsed but
//! not spliced into the emitted struct literal. The field's declared type on
//! the output struct governs the value's type.

use proc_macro::TokenStream;
use proc_macro_error2::abort;
use proc_macro2::TokenStream as TokenStream2;
use quote::{ToTokens, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    Expr, Fields, Ident, ItemStruct, LitStr, Path, Token, Type, TypePath, TypeReference,
    parse_macro_input,
};

use crate::shared::{
    carrier_ident, descriptor_ident, format_const_ident, name_const_ident, pascal_to_snake,
};

/// Klipper-style printf specifier for a given field type.
///
/// Mapping mirrors Klipper's C `DECL_COMMAND` conventions and the wire
/// encoding implemented in `ankyra::encoding`:
///
/// | Rust type | spec  |
/// |-----------|-------|
/// | `u32`     | `%u`  |
/// | `u16`     | `%hu` |
/// | `u8`      | `%c`  |
/// | `i32`     | `%i`  |
/// | `i16`     | `%hi` |
/// | `bool`    | `%c`  |
/// | `&[u8]`   | `%*s` |
/// | `&str`    | `%.*s`|
fn format_spec_for(ty: &Type) -> Option<&'static str> {
    match ty {
        Type::Path(tp) => format_spec_for_primitive(tp),
        Type::Reference(tr) => format_spec_for_reference(tr),
        _ => None,
    }
}

fn format_spec_for_primitive(tp: &TypePath) -> Option<&'static str> {
    if tp.qself.is_some() {
        return None;
    }
    let ident = tp.path.get_ident()?.to_string();
    Some(match ident.as_str() {
        "u32" => "%u",
        "u16" => "%hu",
        // `bool` and `u8` share `%c` because Klipper wire-encodes booleans
        // as single bytes.
        "u8" | "bool" => "%c",
        "i32" => "%i",
        "i16" => "%hi",
        _ => return None,
    })
}

fn format_spec_for_reference(tr: &TypeReference) -> Option<&'static str> {
    if tr.mutability.is_some() {
        return None;
    }
    match tr.elem.as_ref() {
        Type::Slice(slice) => match slice.elem.as_ref() {
            Type::Path(tp) if tp.path.is_ident("u8") => Some("%*s"),
            _ => None,
        },
        Type::Path(tp) if tp.path.is_ident("str") => Some("%.*s"),
        _ => None,
    }
}

/// Parsed `#[klipper_output]` attribute arguments.
///
/// Only the `format = "<literal>"` key is recognised; an empty attribute
/// argument list is equivalent to `OutputAttrArgs { format: None }` and
/// triggers synthesis of the message format from field types.
struct OutputAttrArgs {
    format: Option<LitStr>,
}

impl Parse for OutputAttrArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.is_empty() {
            return Ok(Self { format: None });
        }
        let key: Ident = input.parse()?;
        if key != "format" {
            return Err(syn::Error::new(
                key.span(),
                "unknown #[klipper_output] argument; expected `format = \"...\"`",
            ));
        }
        let _eq: Token![=] = input.parse()?;
        let lit: LitStr = input.parse()?;
        // Trailing comma is tolerated but any further tokens are rejected.
        if input.peek(Token![,]) {
            let _: Token![,] = input.parse()?;
        }
        if !input.is_empty() {
            return Err(syn::Error::new(
                input.span(),
                "unexpected trailing tokens in #[klipper_output(...)]",
            ));
        }
        Ok(Self { format: Some(lit) })
    }
}

/// Scan a printf-style format string and return the list of `%<spec>`
/// tokens, in order.
///
/// The scanner recognises the same spec set as [`format_spec_for`]:
/// `%u`, `%hu`, `%c`, `%i`, `%hi`, `%*s`, `%.*s`. A literal `%%` escape is
/// skipped. Any `%` followed by an unrecognised specifier is returned as a
/// `None` token so the caller can emit a span-pointed error.
///
/// This is intentionally not a general printf parser — the Klipper wire
/// format uses only this small set of specifiers.
fn extract_format_specs(fmt: &str) -> Vec<Option<&'static str>> {
    let mut out = Vec::new();
    let bytes = fmt.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        // At `%`: advance and inspect the follow bytes.
        let j = i + 1;
        if j >= bytes.len() {
            // Trailing lone `%` — treat as unrecognised.
            out.push(None);
            break;
        }
        match bytes[j] {
            b'%' => {
                // `%%` escape — consume two bytes, emit nothing.
                i = j + 1;
            }
            b'u' => {
                out.push(Some("%u"));
                i = j + 1;
            }
            b'c' => {
                out.push(Some("%c"));
                i = j + 1;
            }
            b'i' => {
                out.push(Some("%i"));
                i = j + 1;
            }
            b'*' => {
                // `%*s`
                if j + 1 < bytes.len() && bytes[j + 1] == b's' {
                    out.push(Some("%*s"));
                    i = j + 2;
                } else {
                    out.push(None);
                    i = j + 1;
                }
            }
            b'h' => {
                // `%hu` or `%hi`
                if j + 1 < bytes.len() {
                    match bytes[j + 1] {
                        b'u' => {
                            out.push(Some("%hu"));
                            i = j + 2;
                        }
                        b'i' => {
                            out.push(Some("%hi"));
                            i = j + 2;
                        }
                        _ => {
                            out.push(None);
                            i = j + 1;
                        }
                    }
                } else {
                    out.push(None);
                    i = j + 1;
                }
            }
            b'.' => {
                // `%.*s`
                if j + 2 < bytes.len() && bytes[j + 1] == b'*' && bytes[j + 2] == b's' {
                    out.push(Some("%.*s"));
                    i = j + 3;
                } else {
                    out.push(None);
                    i = j + 1;
                }
            }
            _ => {
                out.push(None);
                i = j + 1;
            }
        }
    }
    out
}

/// Entry point for `#[klipper_output]` attribute expansion.
pub fn expand_output_attribute(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as OutputAttrArgs);
    let item_struct = parse_macro_input!(item as ItemStruct);
    expand_output_attribute_impl(&args, &item_struct).into()
}

#[allow(clippy::too_many_lines)]
fn expand_output_attribute_impl(args: &OutputAttrArgs, item: &ItemStruct) -> TokenStream2 {
    let struct_name = &item.ident;

    // Only named-field structs are supported; see `reply.rs` for the same
    // rationale.
    let named = match &item.fields {
        Fields::Named(n) => n,
        Fields::Unnamed(_) | Fields::Unit => abort!(
            item.ident,
            "#[klipper_output] requires a struct with named fields; \
             tuple structs and unit structs are not supported"
        ),
    };

    // Validate every field type up front and build the (ident, spec) list in
    // declaration order.
    let mut field_specs: Vec<(Ident, &'static str)> = Vec::with_capacity(named.named.len());
    for field in &named.named {
        let Some(ident) = field.ident.as_ref() else {
            abort!(field, "#[klipper_output] fields must be named");
        };
        let Some(spec) = format_spec_for(&field.ty) else {
            let rendered = field.ty.to_token_stream().to_string();
            abort!(
                field.ty,
                "#[klipper_output] field `{}` has unsupported type `{}`. \
                 Supported types: u8, u16, u32, i16, i32, bool, &[u8], &str.",
                ident,
                rendered
            );
        };
        field_specs.push((ident.clone(), spec));
    }

    // Derive the Klipper-style wire name from the struct ident (see
    // `shared::pascal_to_snake`). A user-supplied `format = "..."` is
    // preserved verbatim — its first token is the wire name the host sees,
    // so the user's literal governs. Only the synthesized format and the
    // descriptor's `protocol_name` are affected by the conversion.
    let protocol_name = pascal_to_snake(&struct_name.to_string());
    let message_format = if let Some(lit) = &args.format {
        let user_fmt = lit.value();
        cross_check_format(lit, &user_fmt, &field_specs, named);
        user_fmt
    } else {
        let mut s = protocol_name.clone();
        for (ident, spec) in &field_specs {
            s.push(' ');
            s.push_str(&ident.to_string());
            s.push('=');
            s.push_str(spec);
        }
        s
    };

    let (impl_generics, ty_generics, where_clause) = item.generics.split_for_impl();

    let field_writes = named.named.iter().map(|f| {
        let ident = f.ident.as_ref().expect("named field");
        let ty = &f.ty;
        quote! {
            <#ty as ::ankyra::encoding::Writable>::write(&self.#ident, output);
        }
    });

    let descriptor_fn_name = descriptor_ident(struct_name);
    let carrier_name = carrier_ident("output", struct_name);
    let format_const_name = format_const_ident("output", struct_name);
    let name_const_name = name_const_ident("output", struct_name);

    let output_payload_impl = quote! {
        impl #impl_generics ::ankyra::reply::OutputPayload for #struct_name #ty_generics
        #where_clause {}
    };

    let writable_impl = quote! {
        impl #impl_generics ::ankyra::encoding::Writable for #struct_name #ty_generics
        #where_clause {
            fn write(&self, output: &mut impl ::ankyra::OutputBuffer) {
                #(#field_writes)*
            }
        }
    };

    let descriptor_fn = quote! {
        #[doc(hidden)]
        #[allow(non_snake_case)]
        pub const fn #descriptor_fn_name() -> ::ankyra::descriptor::OutputDescriptor {
            ::ankyra::descriptor::OutputDescriptor::new(#protocol_name, #message_format)
        }
    };

    // Sibling `pub const`s the dictionary builder refers to by
    // reconstructed path. See `shared::format_const_ident` for why.
    let name_const = quote! {
        #[doc(hidden)]
        #[allow(non_upper_case_globals)]
        pub const #name_const_name: &str = #protocol_name;
    };
    let format_const = quote! {
        #[doc(hidden)]
        #[allow(non_upper_case_globals)]
        pub const #format_const_name: &str = #message_format;
    };

    // Carrier macro. Multi-dispatch shape — see reply.rs for rationale.
    //   (kind)            -> "output"
    //   (name)            -> "<protocol_name>"
    //   (format)          -> "<Klipper format string>"
    //   (descriptor_path) -> $crate::<descriptor_fn>
    //   (struct_path)     -> $crate::<Struct>
    //   ()                -> full tuple
    let carrier = quote! {
        #[doc(hidden)]
        #[macro_export]
        macro_rules! #carrier_name {
            (kind) => { "output" };
            (name) => { #protocol_name };
            (format) => { #message_format };
            (descriptor_path) => { $crate::#descriptor_fn_name };
            (struct_path) => { $crate::#struct_name };
            () => {
                (
                    output,
                    #protocol_name,
                    #message_format,
                    $crate::#descriptor_fn_name,
                    $crate::#struct_name,
                )
            };
        }
    };

    quote! {
        #item
        #output_payload_impl
        #writable_impl
        #descriptor_fn
        #name_const
        #format_const
        #carrier
    }
}

/// Cross-check a user-supplied `format = "..."` string against the declared
/// field types.
///
/// The scanner extracts `%<spec>` tokens from the format string in order.
/// Each is compared against the corresponding field's `format_spec_for`
/// value; mismatches abort expansion on the *field* span (more actionable
/// than the literal because the user typically fixes the field type or
/// renames the field).
///
/// If the user supplies a different count of specifiers than the field
/// count, the error points at the literal (the field count is structural —
/// renaming or renumbering fields is the more likely fix).
fn cross_check_format(
    lit: &LitStr,
    fmt: &str,
    field_specs: &[(Ident, &'static str)],
    named: &syn::FieldsNamed,
) {
    let extracted = extract_format_specs(fmt);

    // Walk extracted specs and field_specs in lockstep; fail fast on the
    // first mismatch.
    let mut unrecognised_idx: Option<usize> = None;
    for (idx, slot) in extracted.iter().enumerate() {
        if slot.is_none() {
            unrecognised_idx = Some(idx);
            break;
        }
    }
    if let Some(idx) = unrecognised_idx {
        abort!(
            lit,
            "#[klipper_output(format = ...)]: unrecognised printf specifier at position {} in \
             format string `{}`. Supported specifiers: %u, %hu, %c, %i, %hi, %*s, %.*s.",
            idx + 1,
            fmt
        );
    }

    if extracted.len() != field_specs.len() {
        abort!(
            lit,
            "#[klipper_output(format = ...)]: format string has {} specifier(s) but struct has \
             {} field(s); counts must match.",
            extracted.len(),
            field_specs.len()
        );
    }

    for (i, (field_ident, field_spec)) in field_specs.iter().enumerate() {
        let user_spec = extracted[i].expect("unrecognised specifiers short-circuit above");
        if user_spec != *field_spec {
            // Point at the field's type span — the user's likely fix is
            // "widen u16 to u32" or otherwise change the declared type, so
            // the diagnostic underline should land on the type. We name
            // both expected and declared types in the message.
            let field = named
                .named
                .iter()
                .nth(i)
                .expect("field_specs is built 1:1 from named");
            abort!(
                field.ty.span(),
                "#[klipper_output(format = ...)]: format specifier `{}` at position {} does not \
                 match field `{}` of declared type `{}` (which requires `{}`).",
                user_spec,
                i + 1,
                field_ident,
                field.ty.to_token_stream().to_string(),
                field_spec
            );
        }
    }
}

/// A single `name [: type] = expr` entry in a `klipper_output!` invocation.
struct OutputField {
    name: Ident,
    // Documentary — parsed but not spliced. See `reply.rs` for rationale.
    _ty: Option<Type>,
    expr: Expr,
}

impl Parse for OutputField {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: Ident = input.parse()?;
        let ty = if input.peek(Token![:]) {
            let _colon: Token![:] = input.parse()?;
            Some(input.parse::<Type>()?)
        } else {
            None
        };
        let _eq: Token![=] = input.parse()?;
        let expr: Expr = input.parse()?;
        Ok(Self {
            name,
            _ty: ty,
            expr,
        })
    }
}

/// Parsed `klipper_output!(Path, field1 [: ty] = expr, field2 [: ty] = expr, ...)`.
struct OutputCallSite {
    output_path: Path,
    fields: Punctuated<OutputField, Token![,]>,
}

impl Parse for OutputCallSite {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let output_path: Path = input.parse()?;
        let fields = if input.peek(Token![,]) {
            let _comma: Token![,] = input.parse()?;
            Punctuated::<OutputField, Token![,]>::parse_terminated(input)?
        } else {
            Punctuated::new()
        };
        Ok(Self {
            output_path,
            fields,
        })
    }
}

/// Entry point for `klipper_output!(...)` fn-like expansion.
pub fn expand_output_call_site(input: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(input as OutputCallSite);
    expand_output_call_site_impl(&parsed).into()
}

fn expand_output_call_site_impl(call: &OutputCallSite) -> TokenStream2 {
    let path = &call.output_path;
    let field_inits = call.fields.iter().map(|f| {
        let name = &f.name;
        let expr = &f.expr;
        quote! { #name: #expr }
    });
    quote! {
        <_ as ::ankyra::SendOutput<#path>>::send(
            __ankyra_sender,
            #path { #(#field_inits),* },
        )
    }
}

#[cfg(test)]
mod format_scanner_tests {
    use super::*;

    #[test]
    fn empty_string_has_no_specs() {
        assert!(extract_format_specs("").is_empty());
    }

    #[test]
    fn plain_text_has_no_specs() {
        assert!(extract_format_specs("hello world").is_empty());
    }

    #[test]
    fn single_u_spec() {
        assert_eq!(extract_format_specs("v=%u"), vec![Some("%u")]);
    }

    #[test]
    fn distinguishes_u_from_hu() {
        assert_eq!(
            extract_format_specs("a=%u b=%hu"),
            vec![Some("%u"), Some("%hu")]
        );
    }

    #[test]
    fn distinguishes_i_from_hi() {
        assert_eq!(
            extract_format_specs("a=%i b=%hi"),
            vec![Some("%i"), Some("%hi")]
        );
    }

    #[test]
    fn recognises_all_supported_specs() {
        assert_eq!(
            extract_format_specs("a=%u b=%hu c=%c d=%i e=%hi f=%*s g=%.*s"),
            vec![
                Some("%u"),
                Some("%hu"),
                Some("%c"),
                Some("%i"),
                Some("%hi"),
                Some("%*s"),
                Some("%.*s"),
            ]
        );
    }

    #[test]
    fn percent_percent_escape_is_skipped() {
        assert_eq!(extract_format_specs("literal %% then %u"), vec![Some("%u")]);
    }

    #[test]
    fn unrecognised_specifier_is_none() {
        let got = extract_format_specs("bad %q here");
        assert_eq!(got, vec![None]);
    }

    #[test]
    fn trailing_percent_is_none() {
        let got = extract_format_specs("bad %");
        assert_eq!(got, vec![None]);
    }

    #[test]
    fn trailing_text_after_last_spec_ignored() {
        assert_eq!(extract_format_specs("a=%u tail text"), vec![Some("%u")]);
    }

    #[test]
    fn partial_dotstar_is_none() {
        // `%.u` is not a valid Klipper spec.
        assert_eq!(extract_format_specs("x=%.u"), vec![None]);
    }

    #[test]
    fn partial_star_is_none() {
        // `%*x` is not a valid Klipper spec.
        assert_eq!(extract_format_specs("x=%*x"), vec![None]);
    }
}

#[cfg(test)]
mod attribute_tests {
    use super::*;
    use quote::quote;

    fn render(ts: &TokenStream2) -> String {
        ts.to_string()
    }

    fn expand_attr_for_test(attr: TokenStream2, input: TokenStream2) -> TokenStream2 {
        let args: OutputAttrArgs = syn::parse2(attr).expect("parse OutputAttrArgs");
        let item: ItemStruct = syn::parse2(input).expect("parse ItemStruct");
        expand_output_attribute_impl(&args, &item)
    }

    #[test]
    fn emits_marker_writable_descriptor_carrier_synthesized_format() {
        let input = quote! {
            pub struct DebugPrint {
                pub value: u32,
                pub label: i16,
            }
        };
        let out = render(&expand_attr_for_test(quote!(), input));
        assert!(
            out.contains(":: ankyra :: reply :: OutputPayload for DebugPrint"),
            "missing OutputPayload impl: {out}"
        );
        assert!(
            out.contains(":: ankyra :: encoding :: Writable for DebugPrint"),
            "missing Writable impl: {out}"
        );
        assert!(
            out.contains("__ankyra_descriptor_DebugPrint"),
            "missing descriptor fn: {out}"
        );
        assert!(
            out.contains("__ankyra_item_output_DebugPrint"),
            "missing carrier macro: {out}"
        );
        // `DebugPrint` is PascalCase so the synthesized wire name is
        // `debug_print`. See `shared::pascal_to_snake`.
        assert!(
            out.contains("\"debug_print value=%u label=%hi\""),
            "wrong synthesized message format: {out}"
        );
    }

    #[test]
    fn explicit_format_is_preserved() {
        let input = quote! {
            pub struct Hello<'a> {
                pub v: u32,
                pub s: &'a str,
            }
        };
        let out = render(&expand_attr_for_test(
            quote!(format = "hello v=%u s=%.*s"),
            input,
        ));
        assert!(
            out.contains("\"hello v=%u s=%.*s\""),
            "user-supplied format not preserved: {out}"
        );
        assert!(
            out.contains("OutputPayload for Hello < 'a >"),
            "lifetime not forwarded to OutputPayload impl: {out}"
        );
    }

    #[test]
    fn generics_forwarded_for_str_field() {
        let input = quote! {
            pub struct Echo<'a> {
                pub msg: &'a str,
            }
        };
        let out = render(&expand_attr_for_test(quote!(), input));
        assert!(
            out.contains("OutputPayload for Echo < 'a >"),
            "lifetime not forwarded to OutputPayload impl: {out}"
        );
        assert!(
            out.contains("Writable for Echo < 'a >"),
            "lifetime not forwarded to Writable impl: {out}"
        );
        assert!(
            out.contains("\"echo msg=%.*s\""),
            "wrong format for &str field: {out}"
        );
    }

    #[test]
    fn multi_specifier_explicit_format() {
        let input = quote! {
            pub struct M {
                pub a: u8,
                pub b: u16,
                pub c: u32,
                pub d: i16,
                pub e: i32,
            }
        };
        let out = render(&expand_attr_for_test(
            quote!(format = "m a=%c b=%hu c=%u d=%hi e=%i"),
            input,
        ));
        assert!(
            out.contains("\"m a=%c b=%hu c=%u d=%hi e=%i\""),
            "multi-specifier format not preserved: {out}"
        );
    }
}

#[cfg(test)]
mod call_site_tests {
    use super::*;
    use quote::quote;

    fn render(ts: &TokenStream2) -> String {
        ts.to_string()
    }

    fn expand_call_site_for_test(input: TokenStream2) -> TokenStream2 {
        let parsed: OutputCallSite = syn::parse2(input).expect("parse OutputCallSite");
        expand_output_call_site_impl(&parsed)
    }

    #[test]
    fn emits_turbofish_send_with_struct_literal() {
        let input = quote! { Tick, count: u32 = 1 };
        let out = render(&expand_call_site_for_test(input));
        assert!(
            out.contains("< _ as :: ankyra :: SendOutput < Tick >> :: send"),
            "missing turbofish SendOutput call: {out}"
        );
        assert!(
            out.contains("__ankyra_sender"),
            "missing __ankyra_sender binding ref: {out}"
        );
        assert!(
            out.contains("Tick { count : 1 }"),
            "struct literal missing/wrong: {out}"
        );
    }

    #[test]
    fn ty_annotation_is_optional_and_documentary() {
        let input = quote! { O, a = 1u32, b: i16 = -2 };
        let out = render(&expand_call_site_for_test(input));
        assert!(
            out.contains("a : 1u32"),
            "first field init missing or wrong: {out}"
        );
        assert!(
            out.contains("b : - 2"),
            "second field init missing or wrong: {out}"
        );
        assert!(
            !out.contains("i16"),
            "type annotation leaked into expansion: {out}"
        );
    }

    #[test]
    fn zero_fields_emits_empty_struct_literal() {
        let input = quote! { Beat };
        let out = render(&expand_call_site_for_test(input));
        assert!(
            out.contains("Beat { }"),
            "empty struct literal missing: {out}"
        );
    }

    #[test]
    fn module_qualified_path_accepted() {
        let input = quote! { foo::Tick, count = 1u32 };
        let out = render(&expand_call_site_for_test(input));
        assert!(
            out.contains("foo :: Tick { count : 1u32 }"),
            "qualified path not rendered as struct literal base: {out}"
        );
        assert!(
            out.contains(":: ankyra :: SendOutput < foo :: Tick >"),
            "qualified path not used in turbofish: {out}"
        );
    }
}
