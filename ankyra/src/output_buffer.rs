//! Output buffer trait and a scratch-pad implementation.
//!
//! Ported from anchor's `output_buffer.rs`. `ScratchOutput` keeps its storage
//! inline as `[u8; MAX_SIZE]` so it stays usable in `no_std` contexts without
//! pulling in a new dependency.

/// Trait for output buffers that can accept encoded data.
///
/// Message builders accept an argument of this type and will output their
/// data into the buffer. The buffer must support simple seeking to a
/// previously retrieved position; this is used when backfilling checksum or
/// length fields.
pub trait OutputBuffer {
    /// The cursor type used to mark positions in the buffer.
    type Cursor: Copy;
    /// Append bytes to the buffer.
    fn output(&mut self, buf: &[u8]);
    /// Retrieve the cursor representing the current write position.
    fn cur_position(&self) -> Self::Cursor;
    /// Replace the byte at the cursor position with a new value.
    fn update(&mut self, cursor: Self::Cursor, value: u8);
    /// Retrieve a reference to all data pushed after the cursor.
    fn data_since(&self, cursor: Self::Cursor) -> &[u8];
    /// Roll back the write position to a previously saved cursor, discarding
    /// all bytes written after that point.
    fn rollback(&mut self, cursor: Self::Cursor);
}

/// A scratch-pad [`OutputBuffer`].
///
/// Uses a statically sized inlined buffer. For serializing multiple messages
/// in a row, the buffer can be [`reset`](Self::reset) between uses.
pub struct ScratchOutput<const MAX_SIZE: usize = 64> {
    buffer: [u8; MAX_SIZE],
    idx: usize,
}

impl<const MAX_SIZE: usize> Default for ScratchOutput<MAX_SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_SIZE: usize> ScratchOutput<MAX_SIZE> {
    /// Create a new, empty buffer.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buffer: [0u8; MAX_SIZE],
            idx: 0,
        }
    }

    /// Retrieve the currently built buffer.
    #[must_use]
    pub fn result(&self) -> &[u8] {
        &self.buffer[..self.idx]
    }

    /// Reset the buffer, clearing any written data.
    pub fn reset(&mut self) {
        self.idx = 0;
    }
}

impl<const MAX_SIZE: usize> OutputBuffer for ScratchOutput<MAX_SIZE> {
    type Cursor = usize;

    fn output(&mut self, buf: &[u8]) {
        let area = &mut self.buffer[self.idx..];
        let len = buf.len().clamp(0, area.len());
        area[..len].copy_from_slice(&buf[..len]);
        self.idx += len;
    }

    fn cur_position(&self) -> Self::Cursor {
        self.idx
    }

    fn update(&mut self, cursor: Self::Cursor, value: u8) {
        if cursor < self.idx {
            if let Some(b) = self.buffer.get_mut(cursor) {
                *b = value;
            }
        }
    }

    fn data_since(&self, cursor: Self::Cursor) -> &[u8] {
        if cursor >= self.idx {
            &[]
        } else {
            &self.buffer[cursor..self.idx]
        }
    }

    fn rollback(&mut self, cursor: Self::Cursor) {
        if cursor <= self.idx {
            self.idx = cursor;
        }
    }
}
