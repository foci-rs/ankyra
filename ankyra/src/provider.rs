//! The handle a provider crate publishes for `ankyra_config!` to name.
//!
//! `ankyra_provider!` emits one [`ProviderRef`] const per provider. The
//! firmware lists those consts in `ankyra_config! { providers = [...] }`,
//! which resolves each to the provider's companion macro and folds in its
//! items.

/// Opaque handle naming a provider in `ankyra_config!`.
#[derive(Copy, Clone, Debug)]
pub struct ProviderRef {
    _private: (),
}

impl ProviderRef {
    /// Build the handle `ankyra_provider!` publishes.
    #[doc(hidden)]
    #[must_use]
    pub const fn __new() -> Self {
        Self { _private: () }
    }
}
