mod command;
mod constant;
mod enumeration;
mod output;
mod provider;
mod reply;
mod shared;
mod static_string;

use proc_macro::TokenStream;
use proc_macro_error2::proc_macro_error;

/// Expand a `#[klipper_command]` attribute.
///
/// See [`command`] for the full expansion contract: handler passthrough,
/// dispatch wrapper with view-trait or concrete context binding, and the
/// `#[macro_export]` carrier consumed by the Task 10 assembler.
#[proc_macro_error]
#[proc_macro_attribute]
pub fn klipper_command(attr: TokenStream, item: TokenStream) -> TokenStream {
    command::expand_command(attr, item)
}

/// Expand a `#[klipper_reply]` attribute.
///
/// See [`reply`] for the full expansion contract: struct passthrough,
/// `ReplyPayload` and `Writable` impls, a `pub const fn` returning the
/// `ReplyDescriptor`, and the `#[macro_export]` carrier consumed by the
/// Task 10 assembler.
#[proc_macro_error]
#[proc_macro_attribute]
pub fn klipper_reply(attr: TokenStream, item: TokenStream) -> TokenStream {
    reply::expand_reply_attribute(attr, item)
}

/// Expand the `klipper_reply!(R, field [: ty] = expr, ...)` call-site macro.
///
/// Rust's proc-macro system reserves a single macro namespace per ident, so
/// the attribute `#[klipper_reply]` above and a fn-like `klipper_reply!`
/// cannot coexist under the same name in one proc-macro crate. This macro
/// is therefore published under an internal name here and re-exported from
/// the `ankyra` crate as `klipper_reply`; users invoke it as
/// `::ankyra::klipper_reply!(...)`. Using a proc-macro (rather than a
/// `macro_rules!` in `ankyra`) is what allows the emitted `__ankyra_sender`
/// reference to resolve against the handler body's scope — declarative
/// macros would resolve it at their own definition site instead.
#[proc_macro_error]
#[proc_macro]
pub fn __klipper_reply_call_site(input: TokenStream) -> TokenStream {
    reply::expand_reply_call_site(input)
}

/// Expand a `#[klipper_output]` attribute.
///
/// See [`output`] for the full expansion contract. The attribute accepts an
/// optional `format = "..."` argument; if omitted, the message format is
/// synthesized from the struct name and field specifiers. When the argument
/// is supplied, the format string is cross-checked against the declared
/// field types — a mismatch aborts expansion with a span-pointed
/// diagnostic.
#[proc_macro_error]
#[proc_macro_attribute]
pub fn klipper_output(attr: TokenStream, item: TokenStream) -> TokenStream {
    output::expand_output_attribute(attr, item)
}

/// Expand the `klipper_output!(O, field [: ty] = expr, ...)` call-site
/// macro.
///
/// Published under an internal name for the same reason
/// `__klipper_reply_call_site` is: proc-macro names share a single
/// namespace per crate, so the attribute `#[klipper_output]` and a fn-like
/// `klipper_output!` cannot coexist under the same name. The `ankyra`
/// crate re-exports this macro as `klipper_output`; users invoke it as
/// `::ankyra::klipper_output!(...)`.
#[proc_macro_error]
#[proc_macro]
pub fn __klipper_output_call_site(input: TokenStream) -> TokenStream {
    output::expand_output_call_site(input)
}

/// Expand a `klipper_enumeration! { ... }` invocation.
///
/// See [`enumeration`] for the full expansion contract: enum decl with
/// `Range` pseudo-variants expanded, `From<Enum>` and `TryFrom<uN>` impls
/// (narrowest width sufficient for the variant count), a descriptor fn
/// whose value encodes the `name=id,...` mapping, and a `#[macro_export]`
/// carrier consumed by the Task 10 assembler.
#[proc_macro_error]
#[proc_macro]
pub fn klipper_enumeration(input: TokenStream) -> TokenStream {
    enumeration::expand_enumeration(input)
}

/// Expand a `#[klipper_constant]` attribute.
///
/// See [`constant`] for the full expansion contract: const passthrough, a
/// `pub const fn __ankyra_descriptor_<NAME>` returning a
/// `DefinitionDescriptor` whose `value` is the stringified literal, and a
/// `#[macro_export]` carrier consumed by the Task 10 assembler. Only `u32`
/// and `&str`-typed consts are accepted.
#[proc_macro_error]
#[proc_macro_attribute]
pub fn klipper_constant(attr: TokenStream, item: TokenStream) -> TokenStream {
    constant::expand_constant(attr, item)
}

/// Expand the `klipper_static_string!("msg")` call-site macro.
///
/// See [`static_string`] for the full contract. Expands to the FNV-1a-hashed
/// path `crate::_ankyra_config::static_strings::__ANKYRA_SS_<hash>`; the
/// firmware build fails at type-check if the literal is not listed in the
/// firmware's `ankyra_config!` static-strings entry. The `ankyra` crate
/// re-exports this macro as `klipper_static_string`; users invoke it as
/// `::ankyra::klipper_static_string!("msg")`.
#[proc_macro_error]
#[proc_macro]
pub fn klipper_static_string(tokens: TokenStream) -> TokenStream {
    static_string::expand_static_string(tokens)
}

/// Expand the `klipper_shutdown!("msg", clock_expr)` call-site macro.
///
/// See [`static_string`] for the full contract. Emits a
/// `SendReply<Shutdown>::send` call with the reason string referenced by
/// its FNV-1a hash path (so the host sees the corresponding static-string
/// ID). Must be invoked from inside a `#[klipper_command]` handler body —
/// Task 5's body-scan already recognises the macro and folds the required
/// sender bound onto the dispatch wrapper's generics. The `ankyra` crate
/// re-exports this macro as `klipper_shutdown`.
#[proc_macro_error]
#[proc_macro]
pub fn klipper_shutdown(tokens: TokenStream) -> TokenStream {
    static_string::expand_shutdown(tokens)
}

/// Expand an `ankyra_provider! { name: P, commands: [...], ... }` invocation.
///
/// See [`provider`] for the full expansion contract: a hidden marker type
/// implementing `ProviderSpec`, a user-facing `pub const P: ProviderRef`,
/// and a `#[macro_export] macro_rules! __ankyra_provider_P` companion
/// macro that participates in the Task 11 CPS fold by appending every
/// item's carrier invocation to the accumulator and tail-calling
/// `__ankyra_fold_providers!`.
#[proc_macro_error]
#[proc_macro]
pub fn ankyra_provider(input: TokenStream) -> TokenStream {
    provider::expand_provider(input)
}

/// Expand `ankyra_reexport_provider!(upstream_crate::PROVIDER_NAME)` into
/// a pair of `pub use` re-exports: one for the user-facing const and one
/// for the companion macro. The companion macro's path collapses to
/// `<first_segment>::__ankyra_provider_<NAME>` because `#[macro_export]`
/// publishes declarative macros at the defining crate's root regardless
/// of the module the `ankyra_provider!` invocation lives in.
#[proc_macro_error]
#[proc_macro]
pub fn ankyra_reexport_provider(input: TokenStream) -> TokenStream {
    provider::expand_reexport(input)
}
