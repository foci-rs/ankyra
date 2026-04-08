#![cfg_attr(not(feature = "std"), no_std)]

pub mod descriptor;
pub mod encoding;
mod fifo_buffer;
mod input_buffer;
mod output_buffer;
pub mod provider;
pub mod reply;
pub mod send;
pub mod shutdown;
pub mod transport;
pub mod transport_output;

pub use fifo_buffer::FifoBuffer;
pub use input_buffer::{InputBuffer, SliceInputBuffer};
pub use output_buffer::{OutputBuffer, ScratchOutput};
pub use send::{SendOutput, SendReply};
pub use shutdown::Shutdown;
pub use transport::{ShutdownState, Transport};
pub use transport_output::TransportOutput;

/// Convenience re-exports for end users.
pub mod prelude {
    pub use crate::descriptor::{
        DefinitionDescriptor, DefinitionKind, ItemKind, MessageDescriptor, OutputDescriptor,
        ReplyDescriptor,
    };
    pub use crate::provider::{ProviderRef, ProviderSpec};
    pub use crate::reply::{OutputPayload, ReplyPayload};
    pub use crate::send::{SendOutput, SendReply};
    pub use crate::shutdown::Shutdown;
}
