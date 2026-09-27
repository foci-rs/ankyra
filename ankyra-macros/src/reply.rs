//! `#[klipper_reply]` attribute and `klipper_reply!` call-site expansion.
//!
//! # `#[klipper_reply]` attribute
//!
//! Applied to a plain struct of the form
//!
//! ```ignore
//! #[klipper_reply]
//! pub struct PingReply {
//!     pub seq: u32,
//!     pub value: i16,
//! }
//! ```
//!
//! and emits, in order:
//!
//! 1. The original struct passthrough — user-controlled derives and attrs are
//!    preserved. No extra derives are forced.
//! 2. `impl ::ankyra::reply::ReplyPayload for <T>` — the marker trait, forwarded
//!    over the struct's generics (including any lifetime carried by
//!    `&[u8]`/`&str` fields).
//! 3. `impl ::ankyra::encoding::Writable for <T>` — iterates fields in
//!    declaration order and delegates to each field type's own `Writable`
//!    impl.
//! 4. `pub const fn __ankyra_descriptor_<T>() -> ReplyDescriptor` — returns a
//!    descriptor with the protocol name and the Klipper-style message format
//!    (`"<name> <field1>=%<spec1> <field2>=%<spec2> ..."`). The format string
//!    is constructed at macro-expansion time from the validated field types.
//! 5. `#[macro_export] macro_rules! __ankyra_item_reply_<T>!` — carrier macro
//!    consumed by the assembler. Tuple shape mirrors the command
//!    carrier: `(reply, protocol_name, message_format, descriptor_fn_path)`.
//!
//! It also emits `impl ::ankyra::ReplyWireSize for <T>`: the sum of each
//! field's worst-case VLQ width, or `None` when a field is `&[u8]`/`&str`.
//! The assembler checks that value against the frame budget.
//!
//! Field types are validated against the same wire-type allowlist as command
//! arguments (primitives, `&[u8]`, `&str`). Unions, enums, and
//! tuple structs are rejected.
//!
//! # `klipper_reply!` call-site macro
//!
//! Parsed as `klipper_reply!(R, field1 [: ty] = expr, field2 [: ty] = expr, ...)`
//! and emits `<_ as ::ankyra::SendReply<R>>::send(__ankyra_sender, R { ... })`.
//! The turbofish forces the trait; `_` lets the compiler infer the sender
//! type from `__ankyra_sender`'s type. Must appear inside a
//! `#[klipper_command]` handler body because the emitted code references
//! `__ankyra_sender`, which the command attribute exposes as an injected
//! formal parameter on the rewritten handler.
//!
//! # Why a proc-macro and not `macro_rules!` for the call-site
//!
//! Declarative `macro_rules!` resolve bare identifiers in the macro body
//! against the macro's *definition* scope, so a `__ankyra_sender` written
//! inside a `macro_rules! klipper_reply` would look for a binding in the
//! `ankyra` crate rather than in the handler body. Proc-macros emit idents
//! tagged with `Span::call_site()`, which resolves against the user's scope
//! and therefore finds `#[klipper_command]`'s injected parameter.
//!
//! Rust's proc-macro system reserves a single macro namespace per ident, so
//! `#[klipper_reply]` and `klipper_reply!` cannot coexist under the same
//! name in one crate. The workaround used here: the call-site proc-macro
//! lives under an internal name (`__klipper_reply_call_site`) and is
//! `pub use`-re-exported from the `ankyra` crate as `klipper_reply`. Users
//! invoke it through the `ankyra` re-export path as `::ankyra::klipper_reply!(...)`;
//! the attribute is imported separately from `ankyra_macros`.
//!
use proc_macro::TokenStream;
use proc_macro_error2::abort;
use proc_macro2::TokenStream as TokenStream2;
use quote::{ToTokens, quote, quote_spanned};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{
    Expr, Fields, GenericParam, Ident, ItemStruct, Path, Token, Type, TypePath, TypeReference,
    parse_macro_input,
};

use crate::shared::{
    carrier_ident_with_lifetimes, descriptor_ident, format_const_ident, item_wire_name,
    name_const_ident, wire_size_impl,
};

pub(crate) fn format_spec_for(ty: &Type) -> Option<&'static str> {
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
        // Klipper wire-encodes booleans as single bytes.
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

pub fn expand_reply_attribute(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_struct = parse_macro_input!(item as ItemStruct);
    expand_reply_attribute_impl(&item_struct).into()
}

pub(crate) fn collect_lifetimes_reject_type_generics(
    item: &ItemStruct,
    attr_name: &str,
) -> Vec<syn::Lifetime> {
    let mut lifetimes = Vec::new();
    for param in &item.generics.params {
        match param {
            GenericParam::Lifetime(lt) => lifetimes.push(lt.lifetime.clone()),
            GenericParam::Type(tp) => abort!(
                tp,
                "#[{}] does not support type-parameterized replies; \
                 only lifetime parameters are allowed",
                attr_name
            ),
            GenericParam::Const(cp) => abort!(
                cp,
                "#[{}] does not support const-parameterized replies; \
                 only lifetime parameters are allowed",
                attr_name
            ),
        }
    }
    lifetimes
}

#[allow(clippy::too_many_lines)]
fn expand_reply_attribute_impl(item: &ItemStruct) -> TokenStream2 {
    let struct_name = &item.ident;

    let named = match &item.fields {
        Fields::Named(n) => n,
        Fields::Unnamed(_) | Fields::Unit => abort!(
            item.ident,
            "#[klipper_reply] requires a struct with named fields; \
             tuple structs and unit structs are not supported"
        ),
    };

    let lifetimes = collect_lifetimes_reject_type_generics(item, "klipper_reply");

    let mut field_specs: Vec<(Ident, &'static str)> = Vec::with_capacity(named.named.len());
    for field in &named.named {
        let ident = field.ident.as_ref().expect("named field");
        let Some(spec) = format_spec_for(&field.ty) else {
            let rendered = field.ty.to_token_stream().to_string();
            abort!(
                field.ty,
                "#[klipper_reply] field `{}` has unsupported type `{}`. \
                 Supported types: u8, u16, u32, i16, i32, bool, &[u8], &str.",
                ident,
                rendered
            );
        };
        field_specs.push((ident.clone(), spec));
    }

    let (impl_generics, ty_generics, where_clause) = item.generics.split_for_impl();

    let protocol_name = item_wire_name(&struct_name.to_string());

    let mut message_format = protocol_name.clone();
    for (ident, spec) in &field_specs {
        message_format.push(' ');
        message_format.push_str(&ident.to_string());
        message_format.push('=');
        message_format.push_str(spec);
    }

    // Fully-qualified `<Ty as Writable>::write` so a missing `Writable` impl
    // is a compile error instead of autoderef resolving some other `write`.
    let field_writes = named.named.iter().map(|f| {
        let ident = f.ident.as_ref().expect("named field");
        let ty = &f.ty;
        quote! {
            <#ty as ::ankyra::encoding::Writable>::write(&self.#ident, output);
        }
    });

    let wire_size_impl = wire_size_impl(item, field_specs.iter().map(|(_, spec)| *spec));

    let descriptor_fn_name = descriptor_ident(struct_name);
    // The assembler reads the lifetime count from the carrier ident and emits
    // the `SendReply<Struct<'a0, ..>>` impl itself; invoking a carrier arm for
    // it from the `ankyra_config!` crate would trip rust-lang/rust#52234.
    let carrier_name = carrier_ident_with_lifetimes("reply", struct_name, lifetimes.len());
    let format_const_name = format_const_ident("reply", struct_name);
    let name_const_name = name_const_ident("reply", struct_name);

    let reply_payload_impl = quote! {
        impl #impl_generics ::ankyra::reply::ReplyPayload for #struct_name #ty_generics
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
        pub const fn #descriptor_fn_name() -> ::ankyra::descriptor::ReplyDescriptor {
            ::ankyra::descriptor::ReplyDescriptor::new(#protocol_name, #message_format)
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
            (kind) => { "reply" };
            (name) => { #protocol_name };
            (format) => { #message_format };
            (descriptor_path) => { $crate::#descriptor_fn_name };
            (struct_path) => { $crate::#struct_name };
            () => {
                (
                    reply,
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
        #reply_payload_impl
        #writable_impl
        #wire_size_impl
        #descriptor_fn
        #name_const
        #format_const
        #carrier
    }
}

pub(crate) fn checked_field_init(name: &Ident, ty: Option<&Type>, expr: &Expr) -> TokenStream2 {
    let Some(ty) = ty else {
        return quote! { #name: #expr };
    };
    let value = quote_spanned! { ty.span()=> ::core::convert::identity::<#ty>(#expr) };
    quote! { #name: #value }
}

/// Parses the optional `: ty` of a call-site field, rejecting `impl Trait` anywhere in it.
pub(crate) fn parse_field_annotation(input: ParseStream) -> syn::Result<Option<Type>> {
    struct FindImplTrait(Option<proc_macro2::Span>);
    impl<'ast> Visit<'ast> for FindImplTrait {
        fn visit_type_impl_trait(&mut self, node: &'ast syn::TypeImplTrait) {
            self.0.get_or_insert(node.span());
        }
    }

    if !input.peek(Token![:]) {
        return Ok(None);
    }
    let _colon: Token![:] = input.parse()?;
    let ty: Type = input.parse()?;
    let mut finder = FindImplTrait(None);
    finder.visit_type(&ty);
    if let Some(span) = finder.0 {
        return Err(syn::Error::new(
            span,
            "call-site field annotations must name the field's type; `impl Trait` is not supported",
        ));
    }
    Ok(Some(ty))
}

struct ReplyField {
    name: Ident,
    ty: Option<Type>,
    expr: Expr,
}

impl Parse for ReplyField {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: Ident = input.parse()?;
        let ty = parse_field_annotation(input)?;
        let _eq: Token![=] = input.parse()?;
        let expr: Expr = input.parse()?;
        Ok(Self { name, ty, expr })
    }
}

struct ReplyCallSite {
    reply_path: Path,
    fields: Punctuated<ReplyField, Token![,]>,
}

impl Parse for ReplyCallSite {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let reply_path: Path = input.parse()?;
        let fields = if input.peek(Token![,]) {
            let _comma: Token![,] = input.parse()?;
            Punctuated::<ReplyField, Token![,]>::parse_terminated(input)?
        } else {
            Punctuated::new()
        };
        Ok(Self { reply_path, fields })
    }
}

pub fn expand_reply_call_site(input: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(input as ReplyCallSite);
    expand_reply_call_site_impl(&parsed).into()
}

fn expand_reply_call_site_impl(call: &ReplyCallSite) -> TokenStream2 {
    let path = &call.reply_path;
    let field_inits = call
        .fields
        .iter()
        .map(|f| checked_field_init(&f.name, f.ty.as_ref(), &f.expr));
    quote! {
        <_ as ::ankyra::SendReply<#path>>::send(
            __ankyra_sender,
            #path { #(#field_inits),* },
        )
    }
}

struct ReplyFromCallSite {
    sender_expr: Expr,
    reply_path: Path,
    fields: Punctuated<ReplyField, Token![,]>,
}

impl Parse for ReplyFromCallSite {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let sender_expr: Expr = input.parse()?;
        let _comma: Token![,] = input.parse().map_err(|_| {
            syn::Error::new(
                input.span(),
                "klipper_reply_from! requires a sender expression followed by a comma, \
                 then the reply path, then fields: \
                 klipper_reply_from!(sender_expr, Path, field: ty = expr, ...)",
            )
        })?;
        let reply_path: Path = input.parse()?;
        let fields = if input.peek(Token![,]) {
            let _comma: Token![,] = input.parse()?;
            Punctuated::<ReplyField, Token![,]>::parse_terminated(input)?
        } else {
            Punctuated::new()
        };
        Ok(Self {
            sender_expr,
            reply_path,
            fields,
        })
    }
}

pub fn expand_reply_from_call_site(input: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(input as ReplyFromCallSite);
    expand_reply_from_call_site_impl(&parsed).into()
}

fn expand_reply_from_call_site_impl(call: &ReplyFromCallSite) -> TokenStream2 {
    let sender = &call.sender_expr;
    let path = &call.reply_path;
    let field_inits = call
        .fields
        .iter()
        .map(|f| checked_field_init(&f.name, f.ty.as_ref(), &f.expr));
    quote! {
        {
            let __ankyra_sender = #sender;
            <_ as ::ankyra::SendReply<#path>>::send(
                __ankyra_sender,
                #path { #(#field_inits),* },
            )
        }
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

    fn expand_attr_for_test(input: TokenStream2) -> TokenStream2 {
        let item: ItemStruct = syn::parse2(input).expect("parse ItemStruct");
        expand_reply_attribute_impl(&item)
    }

    #[test]
    fn emits_marker_writable_descriptor_carrier() {
        let input = quote! {
            pub struct PingReply {
                pub seq: u32,
                pub value: i16,
            }
        };
        let out = render(&expand_attr_for_test(input));
        assert!(
            out.contains(":: ankyra :: reply :: ReplyPayload for PingReply"),
            "missing ReplyPayload impl: {out}"
        );
        assert!(
            out.contains(":: ankyra :: encoding :: Writable for PingReply"),
            "missing Writable impl: {out}"
        );
        assert!(
            out.contains("__ankyra_descriptor_PingReply"),
            "missing descriptor fn: {out}"
        );
        assert!(
            out.contains("__ankyra_item_reply_PingReply"),
            "missing carrier macro: {out}"
        );
        assert!(
            out.contains("\"ping_reply seq=%u value=%hi\""),
            "wrong message format: {out}"
        );
    }

    #[test]
    fn generics_forwarded_for_str_field() {
        let input = quote! {
            pub struct Echo<'a> {
                pub msg: &'a str,
            }
        };
        let out = render(&expand_attr_for_test(input));
        assert!(
            out.contains("ReplyPayload for Echo < 'a >"),
            "lifetime not forwarded to ReplyPayload impl: {out}"
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

    fn emitted_max_payload(input: TokenStream2) -> String {
        let out = render(&expand_attr_for_test(input));
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
            let input = quote! { pub struct R { pub v: #ty } };
            assert_eq!(emitted_max_payload(input), expected, "field type {ty}");
        }
    }

    #[test]
    fn max_payload_bytes_sums_mixed_fields() {
        let input = quote! {
            pub struct R { pub a: u8, pub b: bool, pub c: u16, pub d: i16, pub e: u32, pub f: i32 }
        };
        assert_eq!(emitted_max_payload(input), "Some (20)");
    }

    #[test]
    fn max_payload_bytes_none_for_byte_slice() {
        let input = quote! { pub struct R<'a> { pub v: u32, pub data: &'a [u8] } };
        assert_eq!(emitted_max_payload(input), "None");
    }

    #[test]
    fn max_payload_bytes_none_for_str() {
        let input = quote! { pub struct R<'a> { pub v: u32, pub msg: &'a str } };
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
        let parsed: ReplyCallSite = syn::parse2(input).expect("parse ReplyCallSite");
        expand_reply_call_site_impl(&parsed)
    }

    #[test]
    fn emits_turbofish_send_with_struct_literal() {
        let input = quote! { PingReply, seq = 42 };
        let out = render(&expand_call_site_for_test(input));
        // Note the `>>` (no space between the two closes) — `quote!`'s
        // pretty-printer coalesces consecutive angle brackets here.
        assert!(
            out.contains("< _ as :: ankyra :: SendReply < PingReply >> :: send"),
            "missing turbofish SendReply call: {out}"
        );
        assert!(
            out.contains("__ankyra_sender"),
            "missing __ankyra_sender binding ref: {out}"
        );
        assert!(
            out.contains("PingReply { seq : 42 }"),
            "struct literal missing/wrong: {out}"
        );
    }

    #[test]
    fn zero_fields_emits_empty_struct_literal() {
        let input = quote! { Pong };
        let out = render(&expand_call_site_for_test(input));
        assert!(
            out.contains("Pong { }"),
            "empty struct literal missing: {out}"
        );
    }

    #[test]
    fn module_qualified_path_accepted() {
        let input = quote! { foo::PingReply, seq = 1u32 };
        let out = render(&expand_call_site_for_test(input));
        assert!(
            out.contains("foo :: PingReply { seq : 1u32 }"),
            "qualified path not rendered as struct literal base: {out}"
        );
        assert!(
            out.contains(":: ankyra :: SendReply < foo :: PingReply >"),
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
        let parsed: ReplyFromCallSite = syn::parse2(input).expect("parse ReplyFromCallSite");
        expand_reply_from_call_site_impl(&parsed)
    }

    #[test]
    fn binds_sender_expr_then_calls_send() {
        let input = quote! { &mut sender, PingReply, seq = 42u32 };
        let out = render(&expand_from_for_test(input));
        assert!(
            out.contains("let __ankyra_sender = & mut sender"),
            "missing single-evaluation shim: {out}"
        );
        assert!(
            out.contains("< _ as :: ankyra :: SendReply < PingReply >> :: send"),
            "missing turbofish SendReply call: {out}"
        );
        assert!(
            out.contains("PingReply { seq : 42u32 }"),
            "struct literal missing/wrong: {out}"
        );
    }

    #[test]
    fn zero_fields_emits_empty_struct_literal() {
        let input = quote! { s, Pong };
        let out = render(&expand_from_for_test(input));
        assert!(
            out.contains("Pong { }"),
            "empty struct literal missing: {out}"
        );
        assert!(
            out.contains("let __ankyra_sender = s"),
            "missing shim: {out}"
        );
    }

    #[test]
    fn complex_sender_expr_is_bound_once() {
        let input = quote! { transport.sender(), R, a = 1u32 };
        let out = render(&expand_from_for_test(input));
        assert!(
            out.contains("let __ankyra_sender = transport . sender ()"),
            "sender expr not bound: {out}"
        );
        let occurrences = out.matches("__ankyra_sender").count();
        assert_eq!(
            occurrences, 2,
            "expected exactly two references to __ankyra_sender (binding + call site): {out}"
        );
    }
}
