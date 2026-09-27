//! Provider contract: the compile-time interface between a protocol
//! provider crate and the assembler.
//!
//! A provider implements [`ProviderSpec`] on a zero-sized marker type and
//! exposes four static descriptor slices (messages, replies, outputs,
//! definitions). The firmware crate collects providers through
//! `ankyra_config!`, converts each into a type-erased [`ProviderRef`], and
//! hands the result to `__ankyra_assemble!` to produce the dispatch table
//! and data dictionary.

use crate::descriptor::{
    DefinitionDescriptor, MessageDescriptor, OutputDescriptor, ReplyDescriptor,
};

/// Compile-time description of what a provider contributes to the
/// firmware's protocol surface.
///
/// Implementations supply a `MESSAGES` slice and may override the other
/// three defaulted consts as needed. The consts must all be `'static` so
/// the assembler can take references to them during macro expansion.
pub trait ProviderSpec {
    /// Commands, replies, and outputs declared by this provider.
    const MESSAGES: &'static [MessageDescriptor];
    /// Typed reply payloads declared by this provider.
    const REPLIES: &'static [ReplyDescriptor] = &[];
    /// Typed output payloads declared by this provider.
    const OUTPUTS: &'static [OutputDescriptor] = &[];
    /// Constants and enumerations exported by this provider.
    const DEFINITIONS: &'static [DefinitionDescriptor] = &[];
}

/// Type-erased reference to a provider's descriptor tables.
///
/// The firmware-level `ankyra_config!` macro builds one `ProviderRef` per
/// provider via [`ProviderRef::new`], and passes the resulting slice to
/// `__ankyra_assemble!`.
#[derive(Copy, Clone, Debug)]
pub struct ProviderRef {
    messages: &'static [MessageDescriptor],
    replies: &'static [ReplyDescriptor],
    outputs: &'static [OutputDescriptor],
    definitions: &'static [DefinitionDescriptor],
}

impl ProviderRef {
    /// Build the handle `ankyra_provider!` publishes. Carries no tables.
    #[doc(hidden)]
    #[must_use]
    pub const fn __new() -> Self {
        Self {
            messages: &[],
            replies: &[],
            outputs: &[],
            definitions: &[],
        }
    }

    /// Build a `ProviderRef` from a type implementing [`ProviderSpec`].
    #[must_use]
    pub const fn new<P: ProviderSpec>() -> Self {
        Self {
            messages: P::MESSAGES,
            replies: P::REPLIES,
            outputs: P::OUTPUTS,
            definitions: P::DEFINITIONS,
        }
    }

    /// Return this provider's message descriptors.
    #[must_use]
    pub const fn messages(&self) -> &'static [MessageDescriptor] {
        self.messages
    }

    /// Return this provider's reply descriptors.
    #[must_use]
    pub const fn replies(&self) -> &'static [ReplyDescriptor] {
        self.replies
    }

    /// Return this provider's output descriptors.
    #[must_use]
    pub const fn outputs(&self) -> &'static [OutputDescriptor] {
        self.outputs
    }

    /// Return this provider's definition descriptors.
    #[must_use]
    pub const fn definitions(&self) -> &'static [DefinitionDescriptor] {
        self.definitions
    }
}
