//! Simple FIFO byte buffer.
//!
//! Ported from anchor's `fifo_buffer.rs`. Using this is optional; it is
//! provided as a convenience for managing data to and from ankyra protocol
//! handling.

/// FIFO byte buffer backed by an inline array.
pub struct FifoBuffer<const BUF_SIZE: usize> {
    buffer: [u8; BUF_SIZE],
    used: usize,
}

impl<const BUF_SIZE: usize> Default for FifoBuffer<BUF_SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const BUF_SIZE: usize> FifoBuffer<BUF_SIZE> {
    /// Create a new, empty buffer.
    ///
    /// This constructor is `const`, so it can be used in `static` contexts.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buffer: [0u8; BUF_SIZE],
            used: 0,
        }
    }

    /// Returns `true` if the buffer currently holds no data.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.used == 0
    }

    /// Return the length of the currently stored data.
    #[must_use]
    pub fn len(&self) -> usize {
        self.used
    }

    /// Return a mutable slice to the non-filled part of the buffer.
    pub fn receive_buffer(&mut self) -> &mut [u8] {
        &mut self.buffer[self.used..]
    }

    /// Append `buf` to the non-filled part of the buffer.
    ///
    /// If `buf` would overrun the buffer, the call is a no-op.
    pub fn extend(&mut self, buf: &[u8]) {
        let into = self.receive_buffer();
        if into.len() < buf.len() {
            // Drop if we would overrun.
            return;
        }
        into[..buf.len()].copy_from_slice(buf);
        self.used += buf.len();
    }

    /// Move the used cursor forward.
    ///
    /// This can be used after filling part of the non-filled buffer returned
    /// by [`receive_buffer`](Self::receive_buffer).
    pub fn advance(&mut self, n: usize) {
        self.used = (self.used + n).clamp(0, self.buffer.len());
    }

    /// Returns the filled part of the buffer.
    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.buffer[0..self.used]
    }

    /// Remove `n` bytes from the front of the buffer.
    ///
    /// This moves the used part of the buffer down in memory and is therefore
    /// linear in the number of bytes currently stored.
    pub fn pop(&mut self, n: usize) {
        let n = n.clamp(0, self.used);
        let remain = n..self.used;
        let len = remain.len();
        self.buffer.copy_within(remain, 0);
        self.used = len;
    }
}
