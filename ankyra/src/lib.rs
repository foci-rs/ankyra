#![cfg_attr(not(feature = "std"), no_std)]

pub mod encoding;
mod fifo_buffer;
mod input_buffer;
mod output_buffer;

pub use fifo_buffer::FifoBuffer;
pub use input_buffer::{InputBuffer, SliceInputBuffer};
pub use output_buffer::{OutputBuffer, ScratchOutput};
