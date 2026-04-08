mod command;
mod shared;

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
