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
//!    macro consumed by the assembler. Tuple shape mirrors the
//!    reply carrier but with kind ident `output`:
//!    `(output, protocol_name, message_format, descriptor_fn_path)`.
//!
//! It also emits `impl ::ankyra::ReplyWireSize for <T>`: the sum of each
//! field's worst-case VLQ width, or `None` when a field is `&[u8]`/`&str`.
//! The assembler checks that value against the frame budget.
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
use syn::{Expr, Fields, Ident, ItemStruct, LitStr, Path, Token, Type, parse_macro_input};

use crate::reply::{collect_lifetimes_reject_type_generics, field_init, format_spec_for};
use crate::shared::{
    carrier_ident_with_lifetimes, descriptor_ident, format_const_ident, name_const_ident,
    pascal_to_snake, wire_size_impl,
};

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

fn extract_format_specs(fmt: &str) -> Vec<Option<&'static str>> {
    let mut out = Vec::new();
    let bytes = fmt.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        let j = i + 1;
        if j >= bytes.len() {
            out.push(None);
            break;
        }
        match bytes[j] {
            b'%' => i = j + 1,
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
            b'*' if j + 1 < bytes.len() && bytes[j + 1] == b's' => {
                out.push(Some("%*s"));
                i = j + 2;
            }
            b'h' if j + 1 < bytes.len() && bytes[j + 1] == b'u' => {
                out.push(Some("%hu"));
                i = j + 2;
            }
            b'h' if j + 1 < bytes.len() && bytes[j + 1] == b'i' => {
                out.push(Some("%hi"));
                i = j + 2;
            }
            b'.' if j + 2 < bytes.len() && bytes[j + 1] == b'*' && bytes[j + 2] == b's' => {
                out.push(Some("%.*s"));
                i = j + 3;
            }
            _ => {
                out.push(None);
                i = j + 1;
            }
        }
    }
    out
}

pub fn expand_output_attribute(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as OutputAttrArgs);
    let item_struct = parse_macro_input!(item as ItemStruct);
    expand_output_attribute_impl(&args, &item_struct).into()
}

#[allow(clippy::too_many_lines)]
fn expand_output_attribute_impl(args: &OutputAttrArgs, item: &ItemStruct) -> TokenStream2 {
    let struct_name = &item.ident;

    let named = match &item.fields {
        Fields::Named(n) => n,
        Fields::Unnamed(_) | Fields::Unit => abort!(
            item.ident,
            "#[klipper_output] requires a struct with named fields; \
             tuple structs and unit structs are not supported"
        ),
    };

    let lifetimes = collect_lifetimes_reject_type_generics(item, "klipper_output");

    let mut field_specs: Vec<(Ident, &'static str)> = Vec::with_capacity(named.named.len());
    for field in &named.named {
        let ident = field.ident.as_ref().expect("named field");
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

    let wire_size_impl = wire_size_impl(item, field_specs.iter().map(|(_, spec)| *spec));

    let descriptor_fn_name = descriptor_ident(struct_name);
    let carrier_name = carrier_ident_with_lifetimes("output", struct_name, lifetimes.len());
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
        #wire_size_impl
        #descriptor_fn
        #name_const
        #format_const
        #carrier
    }
}

fn cross_check_format(
    lit: &LitStr,
    fmt: &str,
    field_specs: &[(Ident, &'static str)],
    named: &syn::FieldsNamed,
) {
    let extracted = extract_format_specs(fmt);

    if let Some(idx) = extracted.iter().position(Option::is_none) {
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

    for (i, ((field_ident, field_spec), field)) in field_specs.iter().zip(&named.named).enumerate()
    {
        let user_spec = extracted[i].expect("unrecognised specifiers short-circuit above");
        if user_spec != *field_spec {
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

struct OutputField {
    name: Ident,
    ty: Option<Type>,
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
        Ok(Self { name, ty, expr })
    }
}

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

pub fn expand_output_call_site(input: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(input as OutputCallSite);
    expand_output_call_site_impl(&parsed).into()
}

fn expand_output_call_site_impl(call: &OutputCallSite) -> TokenStream2 {
    let path = &call.output_path;
    let field_inits = call
        .fields
        .iter()
        .map(|f| field_init(&f.name, f.ty.as_ref(), &f.expr));
    quote! {
        <_ as ::ankyra::SendOutput<#path>>::send(
            __ankyra_sender,
            #path { #(#field_inits),* },
        )
    }
}

struct OutputFromCallSite {
    sender_expr: Expr,
    output_path: Path,
    fields: Punctuated<OutputField, Token![,]>,
}

impl Parse for OutputFromCallSite {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let sender_expr: Expr = input.parse()?;
        let _comma: Token![,] = input.parse().map_err(|_| {
            syn::Error::new(
                input.span(),
                "klipper_output_from! requires a sender expression followed by a comma, \
                 then the output path, then fields: \
                 klipper_output_from!(sender_expr, Path, field: ty = expr, ...)",
            )
        })?;
        let output_path: Path = input.parse()?;
        let fields = if input.peek(Token![,]) {
            let _comma: Token![,] = input.parse()?;
            Punctuated::<OutputField, Token![,]>::parse_terminated(input)?
        } else {
            Punctuated::new()
        };
        Ok(Self {
            sender_expr,
            output_path,
            fields,
        })
    }
}

pub fn expand_output_from_call_site(input: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(input as OutputFromCallSite);
    expand_output_from_call_site_impl(&parsed).into()
}

fn expand_output_from_call_site_impl(call: &OutputFromCallSite) -> TokenStream2 {
    let sender = &call.sender_expr;
    let path = &call.output_path;
    let field_inits = call
        .fields
        .iter()
        .map(|f| field_init(&f.name, f.ty.as_ref(), &f.expr));
    quote! {
        {
            let __ankyra_sender = #sender;
            <_ as ::ankyra::SendOutput<#path>>::send(
                __ankyra_sender,
                #path { #(#field_inits),* },
            )
        }
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
        assert_eq!(extract_format_specs("x=%.u"), vec![None]);
    }

    #[test]
    fn partial_star_is_none() {
        assert_eq!(extract_format_specs("x=%*x"), vec![None]);
    }
}

#[cfg(test)]
mod attribute_tests {
    use super::*;
    use crate::shared::max_payload_expr;
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

    fn emitted_max_payload(input: TokenStream2) -> String {
        let out = render(&expand_attr_for_test(quote!(), input));
        max_payload_expr(&out)
            .unwrap_or_else(|| panic!("no ReplyWireSize impl: {out}"))
            .to_string()
    }

    #[test]
    fn max_payload_bytes_per_scalar_type() {
        let cases = [
            (quote!(u8), "Some (2)"),
            (quote!(bool), "Some (2)"),
            (quote!(u16), "Some (3)"),
            (quote!(i16), "Some (3)"),
            (quote!(u32), "Some (5)"),
            (quote!(i32), "Some (5)"),
        ];
        for (ty, expected) in cases {
            let input = quote! { pub struct O { pub v: #ty } };
            assert_eq!(emitted_max_payload(input), expected, "field type {ty}");
        }
    }

    #[test]
    fn max_payload_bytes_sums_mixed_fields() {
        let input = quote! {
            pub struct O { pub a: u8, pub b: bool, pub c: u16, pub d: i16, pub e: u32, pub f: i32 }
        };
        assert_eq!(emitted_max_payload(input), "Some (20)");
    }

    #[test]
    fn max_payload_bytes_none_for_byte_slice() {
        let input = quote! { pub struct O<'a> { pub v: u32, pub data: &'a [u8] } };
        assert_eq!(emitted_max_payload(input), "None");
    }

    #[test]
    fn max_payload_bytes_none_for_str() {
        let input = quote! { pub struct O<'a> { pub v: u32, pub msg: &'a str } };
        assert_eq!(emitted_max_payload(input), "None");
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
        let input = quote! { Tick, count = 1 };
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

#[cfg(test)]
mod from_call_site_tests {
    use super::*;
    use quote::quote;

    fn render(ts: &TokenStream2) -> String {
        ts.to_string()
    }

    fn expand_from_for_test(input: TokenStream2) -> TokenStream2 {
        let parsed: OutputFromCallSite = syn::parse2(input).expect("parse OutputFromCallSite");
        expand_output_from_call_site_impl(&parsed)
    }

    #[test]
    fn binds_sender_expr_then_calls_send() {
        let input = quote! { &mut sender, Tick, count = 1u32 };
        let out = render(&expand_from_for_test(input));
        assert!(
            out.contains("let __ankyra_sender = & mut sender"),
            "missing single-evaluation shim: {out}"
        );
        assert!(
            out.contains("< _ as :: ankyra :: SendOutput < Tick >> :: send"),
            "missing turbofish SendOutput call: {out}"
        );
        assert!(
            out.contains("Tick { count : 1u32 }"),
            "struct literal missing/wrong: {out}"
        );
    }

    #[test]
    fn zero_fields_emits_empty_struct_literal() {
        let input = quote! { s, Beat };
        let out = render(&expand_from_for_test(input));
        assert!(
            out.contains("Beat { }"),
            "empty struct literal missing: {out}"
        );
    }

    #[test]
    fn complex_sender_expr_is_bound_once() {
        let input = quote! { transport.sender(), O, a = 1u32 };
        let out = render(&expand_from_for_test(input));
        assert!(
            out.contains("let __ankyra_sender = transport . sender ()"),
            "sender expr not bound: {out}"
        );
        let occurrences = out.matches("__ankyra_sender").count();
        assert_eq!(
            occurrences, 2,
            "expected exactly two references to __ankyra_sender: {out}"
        );
    }
}
