mod command;
mod config;
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
/// See the internal `command` module for the full expansion contract:
/// handler passthrough, dispatch wrapper with view-trait or concrete
/// context binding, and the `#[macro_export]` carrier consumed by the
/// assembler.
///
/// # Errors
///
/// - No first argument → `klipper_command` requires a first context argument
/// - `self` receiver → `klipper_command` does not support `self` receivers
/// - Shared reference context (`&T`) → context argument must be `&mut T` or
///   `&mut dyn Trait`; shared references are not allowed
/// - Owned context (`T`) → context argument must be `&mut T` or `&mut dyn Trait`
/// - Argument with unsupported type → argument `<name>` has unsupported type
///   `T` (supported: `u8`, `u16`, `u32`, `i16`, `i32`, `bool`, `&[u8]`, `&str`)
/// - Non-ident argument pattern (e.g. destructuring) → arguments must use a
///   simple ident pattern
///
/// # Module placement
///
/// `#[klipper_command]` can be invoked at the crate root or in any
/// submodule. `ankyra_provider!`'s item list names the item by path
/// (e.g. `commands: [crate::foo::get_clock]`); the provider threads the
/// module prefix into the carrier-wrapper tuple consumed by the
/// assembler.
#[proc_macro_error]
#[proc_macro_attribute]
pub fn klipper_command(attr: TokenStream, item: TokenStream) -> TokenStream {
    command::expand_command(attr, item)
}

/// Expand a `#[klipper_reply]` attribute.
///
/// See the internal `reply` module for the full expansion contract:
/// struct passthrough, `ReplyPayload` and `Writable` impls, a
/// `pub const fn` returning the `ReplyDescriptor`, and the
/// `#[macro_export]` carrier consumed by the assembler.
///
/// # Errors
///
/// - Tuple / unit struct (not named fields) → `#[klipper_reply]` requires a
///   struct with named fields
/// - Unnamed field somehow remaining → fields must be named
/// - Unsupported field type → field `<name>` has unsupported type `T`
///   (allowlist mirrors `#[klipper_command]`)
/// - Two `#[klipper_reply]` structs sharing an ident in the same crate →
///   `E0428` on the emitted `#[macro_export]` carrier and descriptor fn
///
/// # Module placement
///
/// `#[klipper_reply]` can be invoked at the crate root or in any
/// submodule. `ankyra_provider!`'s item list names the item by path
/// (e.g. `replies: [crate::foo::PingReply]`); the provider threads the
/// module prefix into the carrier-wrapper tuple consumed by the
/// assembler.
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
///
/// # Errors
///
/// - Invocation outside a `#[klipper_command]` handler body →
///   `E0425 cannot find value __ankyra_sender in this scope`
/// - Call-site `: ty` that disagrees with the expression or the declared
///   field type → `E0308 mismatched types` at the annotation
/// - First token is not a path to a `#[klipper_reply]` struct → downstream
///   `Writable` / `ReplyPayload` bound errors
#[proc_macro_error]
#[proc_macro]
pub fn __klipper_reply_call_site(input: TokenStream) -> TokenStream {
    reply::expand_reply_call_site(input)
}

/// Expand the `klipper_reply_from!(sender_expr, R, field [: ty] = expr, ...)`
/// call-site macro for non-handler contexts.
///
/// Re-exported from the `ankyra` crate as `klipper_reply_from`. Unlike
/// `__klipper_reply_call_site!`, this form does not rely on an in-scope
/// `__ankyra_sender` binding: the caller passes the sender (or any `&mut T`
/// implementing `::ankyra::SendReply<R>`) as the first argument. The
/// expression is evaluated exactly once.
///
/// # Errors
///
/// - Missing sender expression / missing comma after it → parse error
///   pointing at the macro invocation
/// - First token is not a valid expression → parse error from `syn::Expr`
/// - Second token is not a path to a `#[klipper_reply]` struct → downstream
///   `Writable` / `ReplyPayload` bound errors
/// - Call-site `: ty` that disagrees with the expression or the declared
///   field type → `E0308 mismatched types` at the annotation
/// - Sender argument does not implement `::ankyra::SendReply<R>` → `E0277`
///   trait bound not satisfied at the emitted dispatch call
#[proc_macro_error]
#[proc_macro]
pub fn __klipper_reply_from_call_site(input: TokenStream) -> TokenStream {
    reply::expand_reply_from_call_site(input)
}

/// Expand a `#[klipper_output]` attribute.
///
/// See the internal `output` module for the full expansion contract. The
/// attribute accepts an optional `format = "..."` argument; if omitted,
/// the message format is synthesized from the struct name and field
/// specifiers. When the argument
/// is supplied, the format string is cross-checked against the declared
/// field types — a mismatch aborts expansion with a span-pointed
/// diagnostic.
///
/// # Errors
///
/// - Tuple / unit struct → `#[klipper_output]` requires a struct with named
///   fields
/// - Unsupported field type → field `<name>` has unsupported type `T` (same
///   allowlist as replies)
/// - `format = "..."` with an unknown key → parse error on the attribute arg
/// - `format = "..."` placeholder count or spec mismatch → format string
///   placeholder does not match field `<name>` of type `T`
///
/// # Module placement
///
/// `#[klipper_output]` can be invoked at the crate root or in any
/// submodule. `ankyra_provider!`'s item list names the item by path
/// (e.g. `outputs: [crate::foo::DebugPrint]`); the provider threads the
/// module prefix into the carrier-wrapper tuple consumed by the
/// assembler.
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
///
/// # Errors
///
/// - Invocation outside a `#[klipper_command]` handler body →
///   `E0425 cannot find value __ankyra_sender in this scope`
/// - Call-site `: ty` that disagrees with the expression or the declared
///   field type → `E0308 mismatched types` at the annotation
/// - First token is not a path to a `#[klipper_output]` struct → downstream
///   `Writable` / `OutputPayload` bound errors
#[proc_macro_error]
#[proc_macro]
pub fn __klipper_output_call_site(input: TokenStream) -> TokenStream {
    output::expand_output_call_site(input)
}

/// Expand the `klipper_output_from!(sender_expr, O, field [: ty] = expr, ...)`
/// call-site macro for non-handler contexts.
///
/// Re-exported from the `ankyra` crate as `klipper_output_from`. Mirrors
/// `__klipper_reply_from_call_site!` but for output payloads: the caller
/// supplies a `&mut T` implementing `::ankyra::SendOutput<O>` as the first
/// argument. The expression is evaluated exactly once.
///
/// # Errors
///
/// - Missing sender expression / missing comma after it → parse error
///   pointing at the macro invocation
/// - First token is not a valid expression → parse error from `syn::Expr`
/// - Second token is not a path to a `#[klipper_output]` struct → downstream
///   `Writable` / `OutputPayload` bound errors
/// - Call-site `: ty` that disagrees with the expression or the declared
///   field type → `E0308 mismatched types` at the annotation
/// - Sender argument does not implement `::ankyra::SendOutput<O>` → `E0277`
///   trait bound not satisfied at the emitted dispatch call
#[proc_macro_error]
#[proc_macro]
pub fn __klipper_output_from_call_site(input: TokenStream) -> TokenStream {
    output::expand_output_from_call_site(input)
}

/// Expand a `klipper_enumeration! { ... }` invocation.
///
/// See the internal `enumeration` module for the full expansion
/// contract: enum decl with `Range` pseudo-variants expanded,
/// `From<Enum>` and `TryFrom<uN>` impls (narrowest width sufficient for
/// the variant count), a descriptor fn
/// whose value encodes the `name=id,...` mapping, and a `#[macro_export]`
/// carrier consumed by the assembler.
///
/// # Errors
///
/// - Unknown key in the `(...)` header → unknown `klipper_enumeration`
///   header option (expected `name = "..."` or `rename_all = "..."`)
/// - Unknown `rename_all` value → unknown `rename_all` value (expected one
///   of: `lowercase`, `UPPERCASE`, `snake_case`, `SCREAMING_SNAKE_CASE`,
///   `camelCase`, `PascalCase`, `kebab-case`)
/// - Per-variant attribute with an unknown key → unknown
///   `klipper_enumeration` variant option (expected `rename = "..."`)
/// - `Range(prefix, start, 0)` → `Range` count must be at least 1
///
/// # Module placement
///
/// `klipper_enumeration!` can be invoked at the crate root or in any
/// submodule. `ankyra_provider!`'s item list names the item by path
/// (e.g. `enumerations: [crate::foo::MotorKind]`); the provider threads
/// the module prefix into the carrier-wrapper tuple consumed by the
/// assembler.
#[proc_macro_error]
#[proc_macro]
pub fn klipper_enumeration(input: TokenStream) -> TokenStream {
    enumeration::expand_enumeration(input)
}

/// Expand a `#[klipper_constant]` attribute.
///
/// See the internal `constant` module for the full expansion contract:
/// const passthrough, a `pub const fn __ankyra_descriptor_<NAME>` returning a
/// `DefinitionDescriptor` whose `value` is the stringified literal, and a
/// `#[macro_export]` carrier consumed by the assembler. Only `u32` and
/// `&str`-typed consts are accepted.
///
/// # Errors
///
/// - Non-const item (fn, struct, …) → `#[klipper_constant]` only applies to
///   `const` items
/// - Unsupported type (anything other than `u32` or `&'static str`) →
///   `klipper_constant` type must be `u32` or `&'static str`
/// - Non-literal initializer → `klipper_constant` initializer must be a
///   literal
///
/// # Module placement
///
/// `#[klipper_constant]` can be invoked at the crate root or in any
/// submodule. `ankyra_provider!`'s item list names the item by path
/// (e.g. `constants: [crate::foo::FREQ]`); the provider threads the
/// module prefix into the carrier-wrapper tuple consumed by the
/// assembler.
#[proc_macro_error]
#[proc_macro_attribute]
pub fn klipper_constant(attr: TokenStream, item: TokenStream) -> TokenStream {
    constant::expand_constant(attr, item)
}

/// Expand the `klipper_static_string!("msg")` call-site macro.
///
/// See the internal `static_string` module for the full contract. Expands
/// to the FNV-1a-hashed path
/// `crate::_ankyra_config::static_strings::__ANKYRA_SS_<hash>`; the
/// firmware build fails at type-check if the literal is not listed in the
/// firmware's `ankyra_config!` static-strings entry. The `ankyra` crate
/// re-exports this macro as `klipper_static_string`; users invoke it as
/// `::ankyra::klipper_static_string!("msg")`.
///
/// # Errors
///
/// - Argument is not a string literal → parse error (message span points at
///   the offending token)
/// - Literal not listed in this crate's
///   `ankyra_config! { static_strings = [...] }` →
///   `E0425 cannot find value __ANKYRA_SS_<hash> in module
///   crate::_ankyra_config::static_strings`
/// - No `ankyra_config!` invocation in this crate at all →
///   `E0433 failed to resolve: could not find _ankyra_config in the crate
///   root`
#[proc_macro_error]
#[proc_macro]
pub fn klipper_static_string(tokens: TokenStream) -> TokenStream {
    static_string::expand_static_string(tokens)
}

/// Expand the `klipper_shutdown!("msg", clock_expr)` call-site macro.
///
/// See the internal `static_string` module for the full contract. Emits a
/// `SendReply<Shutdown>::send` call with the reason string referenced by
/// its FNV-1a hash path (so the host sees the corresponding static-string
/// ID). Must be invoked from inside a `#[klipper_command]` handler body —
/// the handler body-scan recognises the macro and folds the required
/// sender bound onto the dispatch wrapper's generics. The `ankyra` crate
/// re-exports this macro as `klipper_shutdown`.
///
/// # Errors
///
/// - No arguments → `klipper_shutdown!` requires two arguments (a string
///   literal message and a clock expression)
/// - Missing clock expression → same message (span points at the message arg)
/// - More than two arguments → `klipper_shutdown!` accepts exactly two
///   arguments (span points at the extra arg)
/// - First argument is not a string literal → `klipper_shutdown!` first
///   argument must be a string literal
/// - Invocation outside a `#[klipper_command]` handler body →
///   `E0425 cannot find value __ankyra_sender in this scope`
/// - Reason literal not listed in this crate's
///   `ankyra_config! { static_strings = [...] }` →
///   `E0425 cannot find value __ANKYRA_SS_<hash> in module
///   crate::_ankyra_config::static_strings`
#[proc_macro_error]
#[proc_macro]
pub fn klipper_shutdown(tokens: TokenStream) -> TokenStream {
    static_string::expand_shutdown(tokens)
}

/// Expand the `klipper_shutdown_from!(sender_expr, "msg", clock_expr)`
/// call-site macro for non-handler contexts.
///
/// Re-exported from the `ankyra` crate as `klipper_shutdown_from`. Mirrors
/// `__klipper_reply_from_call_site!` and `__klipper_output_from_call_site!`
/// but for the firmware-wide `::ankyra::Shutdown` reply: the caller supplies
/// a `&mut T` implementing `::ankyra::SendReply<::ankyra::Shutdown>` as the
/// first argument, followed by the static-string reason literal and the
/// clock expression. The sender expression is evaluated exactly once.
///
/// The reason literal is FNV-1a-hashed using the same routine as
/// `klipper_static_string!` / `klipper_shutdown!`; the firmware build fails
/// unless the literal is listed in `ankyra_config! { static_strings = [...] }`.
/// The expansion intentionally does *not* depend on `#[klipper_command]`'s
/// body-scan folding a `SendReply<Shutdown>` bound onto a wrapper generic —
/// the bound is satisfied by the explicit sender argument instead, which is
/// what makes this form usable from RTIC tasks, timer callbacks, NVIC
/// interrupt handlers, or any other non-handler code path.
///
/// # Errors
///
/// - Missing sender expression / missing comma after it → parse error
///   pointing at the macro invocation with a usage hint
/// - Second argument is not a string literal → parse error (sender first,
///   then literal reason)
/// - Missing clock expression → parse error pointing after the reason literal
/// - More than three arguments → parse error on the trailing token
/// - Reason literal not listed in this crate's
///   `ankyra_config! { static_strings = [...] }` →
///   `E0425 cannot find value __ANKYRA_SS_<hash> in module
///   crate::_ankyra_config::static_strings`
/// - Sender argument does not implement
///   `::ankyra::SendReply<::ankyra::Shutdown>` → `E0277` trait bound not
///   satisfied at the emitted dispatch call
#[proc_macro_error]
#[proc_macro]
pub fn __klipper_shutdown_from_call_site(input: TokenStream) -> TokenStream {
    static_string::expand_shutdown_from_call_site(input)
}

/// Expand an `ankyra_provider! { name: P, commands: [...], ... }` invocation.
///
/// See the internal `provider` module for the full expansion contract: a
/// hidden marker type implementing `ProviderSpec`, a user-facing
/// `pub const P: ProviderRef`,
/// and a `#[macro_export] macro_rules! __ankyra_provider_P` companion
/// macro that participates in the `ankyra_config!` CPS fold by appending every
/// item's carrier invocation to the accumulator and tail-calling
/// `__ankyra_fold_providers!`.
///
/// # Errors
///
/// - Missing `name:` key → `ankyra_provider!` requires a `name:` key
/// - Unknown key (anything outside `name`, `commands`, `replies`, `outputs`,
///   `constants`, `enumerations`) → unknown `ankyra_provider!` key `<key>`
/// - Duplicate key (e.g. two `commands:` entries) → duplicate `<key>` key
/// - Duplicate ident within a list (e.g. `commands: [a, a]`) → duplicate
///   entry `<ident>` in `ankyra_provider!` list
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
///
/// # Errors
///
/// - Trailing tokens after the provider path → `ankyra_reexport_provider!`
///   takes a single provider path argument
/// - Path without a last segment identifier → invalid provider path
///   (expected an identifier in the final segment)
#[proc_macro_error]
#[proc_macro]
pub fn ankyra_reexport_provider(input: TokenStream) -> TokenStream {
    provider::expand_reexport(input)
}

/// Expand an `ankyra_config! { transport = .., context = .., providers = [..], static_strings = [..] }`
/// invocation into the CPS-fold entry point.
///
/// See the internal `config` module for the full expansion contract: the
/// shim is a thin launcher that hands `config = { ... }`, an empty
/// accumulator, and the list of rewritten provider companion macro paths to
/// `::ankyra::__ankyra_fold_providers!`. The fold walks the provider list,
/// each provider's companion macro appending its carrier tuples to the
/// accumulator, until `remaining` is empty and the accumulated items reach
/// `::ankyra_assemble::__ankyra_assemble!` — which emits the
/// `KLIPPER_TRANSPORT` binding, the dispatch table, and the data
/// dictionary.
///
/// The `ankyra` crate re-exports this macro so users invoke it as
/// `::ankyra::ankyra_config! { ... }`.
///
/// # Errors
///
/// - Missing `transport`, `context`, or `providers` → `ankyra_config!`
///   requires `<key> = ...`
/// - Unknown key (outside `transport`, `context`, `providers`,
///   `static_strings`) → unknown `ankyra_config!` key `<key>`
/// - Duplicate key → duplicate `<key>` key
/// - Bare ident in `providers` (e.g. `providers = [PROVIDER]` instead of
///   `providers = [crate::PROVIDER]`) → parse error pointing at the bare
///   ident
/// - Static-string literal used with `klipper_static_string!` or
///   `klipper_shutdown!` but not listed here →
///   `E0425 cannot find value __ANKYRA_SS_<hash>` at the call site
/// - Protocol-name collision across items from different providers →
///   detected by the assembler (message: protocol name defined twice)
/// - Handler body uses a payload for which the dispatch wrapper's generics
///   don't satisfy `SendReply<R>` → trait bound not satisfied at the
///   emitted dispatch call
#[proc_macro_error]
#[proc_macro]
pub fn ankyra_config(input: TokenStream) -> TokenStream {
    config::expand_ankyra_config(input)
}
