//! Runtime helpers for the Klipper data dictionary served by
//! `identify_response`.
//!
//! Klipper's host runs `zlib.decompress()` on the bytes the firmware
//! returns from the `identify` command, so we must deliver the
//! dictionary wrapped in an RFC 1950 zlib stream (2-byte header +
//! deflate payload + 4-byte Adler-32 trailer). The dictionary itself is
//! assembled at const-eval time by the `ankyra-assemble` macro (see
//! `DICT_BYTES`), so compression is the only step that runs at
//! firmware runtime.
//!
//! # Why stored-block deflate
//!
//! ankyra ships `no_std`-first and its first consumer (FOCI) runs
//! without a global allocator. The mainstream Rust deflate crates
//! (`miniz_oxide`, `flate2`, `libflate`, `yazi`) all require `alloc`
//! for their encoder state (Huffman tables alone are ~300 KB when
//! dynamic). A proper deflate encoder is not buildable on a pure
//! stack budget without an allocator.
//!
//! Klipper does not need the dictionary to be tightly compressed — it
//! just runs `zlib.decompress()` on whatever bytes we send and caches
//! the result. A DEFLATE "stored block" (BTYPE = 00, RFC 1951
//! §3.2.4) is an uncompressed literal block wrapped in 5 bytes of
//! framing. Chaining stored blocks up to 65535 bytes each and
//! prefixing the RFC 1950 zlib header (+ Adler-32 trailer) produces a
//! valid, round-trip-safe zlib stream with zero compression but also
//! zero allocator use.
//!
//! The resulting payload is roughly `uncompressed_len + 11 + 5 *
//! blocks` bytes — for typical firmware dictionaries (~1–2 KB JSON)
//! the overhead is negligible and the wire contract is satisfied.
//!
//! If a future revision picks up true deflate compression (e.g. for
//! very large dictionaries) the signatures of [`compress_dict_to`] and
//! [`max_compressed_size`] are stable: callers can keep the same stack
//! scratch buffer and the implementation can grow underneath.
//!
//! # Why on-demand (not cached)
//!
//! `identify` is rare (typically once per host connection) and the
//! compressed dictionary is small, so the assembler-generated
//! `handle_identify` helper re-compresses on every call into a stack
//! buffer. This keeps the implementation allocator-free without
//! needing a one-time-init primitive like `OnceCell` or
//! `critical-section`.

/// Errors returned by [`compress_dict_to`].
///
/// Compression is a pure state-machine crunch over the input buffer —
/// the only failure mode that is physically possible here is running
/// out of room in the caller-supplied output buffer. We keep the enum
/// exhaustive anyway so future additions (e.g. a move to a real
/// deflate encoder with internal error modes) do not silently widen
/// the existing variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CompressError {
    /// The caller-supplied output buffer was too small to hold the
    /// compressed dictionary. Size with [`max_compressed_size`].
    OutputTooSmall,
}

/// Maximum payload size for a single DEFLATE stored block (RFC 1951
/// §3.2.4). The length field is a 16-bit little-endian value so a
/// single block cannot carry more than 65535 bytes.
const STORED_BLOCK_MAX: usize = 0xFFFF;

/// Compress `input` into `output` with zlib framing (RFC 1950).
///
/// The payload is a chain of uncompressed stored blocks (DEFLATE
/// BTYPE = 00) prefixed by the zlib header and suffixed by the
/// Adler-32 trailer. See the module-level docs for why stored blocks.
///
/// Returns the number of bytes actually written into `output` on
/// success. Size `output` with [`max_compressed_size`] so the only
/// failure mode in practice is invariant-level.
///
/// # Errors
///
/// Returns [`CompressError::OutputTooSmall`] if `output` is not large
/// enough to hold the compressed stream.
pub fn compress_dict_to(input: &[u8], output: &mut [u8]) -> Result<usize, CompressError> {
    let needed = max_compressed_size(input.len());
    if output.len() < needed {
        return Err(CompressError::OutputTooSmall);
    }
    let mut w = 0usize;

    // RFC 1950 zlib header: CMF = 0x78 (deflate with 32 KiB window);
    // FLG chosen so the u16be of (CMF, FLG) is a multiple of 31 and
    // the FLEVEL field signals "fastest" (we emit stored blocks, so
    // there is no real compression level to advertise). `0x01`
    // satisfies both constraints: `(0x78 << 8) | 0x01 = 30721` and
    // `30721 % 31 == 0`. `FDICT` is not set.
    output[w] = 0x78;
    output[w + 1] = 0x01;
    w += 2;

    // DEFLATE: chain of stored blocks. Each block is:
    //   1 byte : BFINAL (bit 0) | BTYPE = 00 (bits 1..2), padded
    //   2 bytes: LEN  (little-endian)
    //   2 bytes: NLEN (one's complement of LEN)
    //   LEN bytes of literal data.
    // An empty input still needs one terminating block.
    if input.is_empty() {
        output[w] = 0x01; // BFINAL=1, BTYPE=00
        output[w + 1] = 0x00;
        output[w + 2] = 0x00;
        output[w + 3] = 0xFF;
        output[w + 4] = 0xFF;
        w += 5;
    } else {
        let mut remaining = input;
        while !remaining.is_empty() {
            let chunk_len = remaining.len().min(STORED_BLOCK_MAX);
            let is_last = chunk_len == remaining.len();
            // `u16` fits because `chunk_len <= STORED_BLOCK_MAX`.
            #[allow(clippy::cast_possible_truncation)]
            let len_u16 = chunk_len as u16;
            output[w] = u8::from(is_last); // BFINAL in bit 0, BTYPE = 00
            w += 1;
            output[w..w + 2].copy_from_slice(&len_u16.to_le_bytes());
            w += 2;
            output[w..w + 2].copy_from_slice(&(!len_u16).to_le_bytes());
            w += 2;
            output[w..w + chunk_len].copy_from_slice(&remaining[..chunk_len]);
            w += chunk_len;
            remaining = &remaining[chunk_len..];
        }
    }

    // RFC 1950 trailer: Adler-32 of the uncompressed input, big-endian.
    let adler = adler32(input);
    output[w..w + 4].copy_from_slice(&adler.to_be_bytes());
    w += 4;

    Ok(w)
}

/// Upper bound on the compressed output size for a given input length.
///
/// Chosen so firmware can size the `handle_identify` stack scratch
/// buffer at compile time:
///
/// ```ignore
/// let mut scratch = [0u8; ::ankyra::dictionary::max_compressed_size(DICT_BYTES.len())];
/// ```
///
/// The bound covers the worst case for our stored-block encoder:
///
/// * 2 bytes for the zlib header.
/// * 5 bytes of framing per stored block, plus 5 bytes for an
///   always-emitted terminating block when `uncompressed_len == 0`.
/// * `uncompressed_len` bytes of literal payload.
/// * 4 bytes for the Adler-32 trailer.
#[must_use]
pub const fn max_compressed_size(uncompressed_len: usize) -> usize {
    // Ceiling-division of `uncompressed_len` by `STORED_BLOCK_MAX`,
    // with a floor of 1 so an empty input still reserves space for the
    // mandatory terminating block.
    let blocks = if uncompressed_len == 0 {
        1
    } else {
        uncompressed_len.div_ceil(STORED_BLOCK_MAX)
    };
    // 2 header + 5 per block framing + payload + 4 trailer.
    2 + 5 * blocks + uncompressed_len + 4
}

/// Adler-32 checksum per RFC 1950 §9.
///
/// Uses the straightforward O(n) accumulator; no table, no unrolling.
/// Firmware dictionary sizes are ~1 KB so the simple form is fast
/// enough to run per-identify.
fn adler32(input: &[u8]) -> u32 {
    // `MOD_ADLER` is the largest prime below 65536 — RFC 1950 mandates
    // the Adler-32 reduction modulo this prime.
    const MOD_ADLER: u32 = 65521;
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in input {
        a = (a + u32::from(byte)) % MOD_ADLER;
        b = (b + a) % MOD_ADLER;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::*;

    /// If the output buffer cannot fit the compressed stream the
    /// helper returns `OutputTooSmall` rather than silently
    /// truncating.
    #[test]
    fn too_small_output_returns_error() {
        let input = b"hello";
        let mut tiny = [0u8; 2];
        assert_eq!(
            compress_dict_to(input, &mut tiny),
            Err(CompressError::OutputTooSmall)
        );
    }

    /// `max_compressed_size` must be usable as a `const` expression so
    /// firmware can size stack buffers at compile time.
    #[test]
    fn max_compressed_size_is_const_evaluable() {
        const BOUND_EMPTY: usize = max_compressed_size(0);
        const BOUND_SMALL: usize = max_compressed_size(1024);
        const BOUND_LARGE: usize = max_compressed_size(100_000);
        assert_eq!(BOUND_EMPTY, 2 + 5 + 4);
        assert_eq!(BOUND_SMALL, 2 + 5 + 1024 + 4);
        // 100_000 spans two 65535-byte blocks, so 2 * 5 bytes of framing.
        assert_eq!(BOUND_LARGE, 2 + 10 + 100_000 + 4);
    }

    /// Adler-32 test vector from RFC 1950: `adler32("")` = 1.
    #[test]
    fn adler32_empty_is_one() {
        assert_eq!(adler32(&[]), 1);
    }

    /// Adler-32 test vector: `adler32(b"Wikipedia")` = 0x11E60398 (well-known
    /// published value).
    #[test]
    fn adler32_known_vector() {
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    /// Basic smoke: emits the zlib magic byte. Deeper round-trip
    /// verification (which needs `flate2::read::ZlibDecoder` and
    /// therefore `std`) lives in the `compression.rs` integration
    /// test file where the test harness already has `std` available
    /// as a dev-dependency.
    #[test]
    fn emits_zlib_magic() {
        let mut scratch = [0u8; 64];
        let n = compress_dict_to(b"x", &mut scratch).expect("fits");
        assert!(n >= 2);
        assert_eq!(scratch[0], 0x78, "zlib magic not emitted");
        let hdr = u16::from_be_bytes([scratch[0], scratch[1]]);
        assert_eq!(hdr % 31, 0, "zlib FCHECK invalid");
    }
}
