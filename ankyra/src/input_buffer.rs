//! Input buffer trait and a slice-backed implementation.

/// Trait representing a buffer that protocol messages can be read from.
pub trait InputBuffer {
    /// Retrieve all currently buffered data.
    fn data(&self) -> &[u8];
    /// Remove `count` bytes from the front of the buffer.
    fn pop(&mut self, count: usize);
    /// Retrieve the amount of data currently in the buffer.
    fn available(&self) -> usize {
        self.data().len()
    }
}

/// An [`InputBuffer`] implementation wrapping a borrowed byte slice.
pub struct SliceInputBuffer<'a> {
    buffer: &'a [u8],
}

impl<'a> SliceInputBuffer<'a> {
    /// Create a new `SliceInputBuffer` backed by an input byte slice.
    #[must_use]
    pub fn new(buffer: &'a [u8]) -> Self {
        Self { buffer }
    }
}

impl InputBuffer for SliceInputBuffer<'_> {
    fn data(&self) -> &[u8] {
        self.buffer
    }

    fn pop(&mut self, count: usize) {
        let count = count.min(self.buffer.len());
        self.buffer = &self.buffer[count..];
    }
}
