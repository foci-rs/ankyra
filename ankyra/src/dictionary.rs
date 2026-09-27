//! Runtime helpers for the Klipper data dictionary served by
//! `identify_response`.
//!
//! Klipper's host runs `zlib.decompress()` on the bytes the firmware
//! returns from the `identify` command, so we must deliver the
//! dictionary wrapped in an RFC 1950 zlib stream (2-byte header +
//! deflate payload + 4-byte Adler-32 trailer). The dictionary itself is
//! assembled at const-eval time by the `ankyra-assemble` macro (see
//! `DICT_BYTES`), and [`compress_dict`] derives the wire bytes at
//! compile time.
//!
//! # Why stored-block deflate
//!
//! ankyra ships `no_std`-first for firmware without a global allocator.
//! The mainstream Rust deflate crates (`miniz_oxide`, `flate2`,
//! `libflate`, `yazi`) all require `alloc` for their encoder state.
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
//! The resulting stream is exactly [`compressed_size`] bytes: the
//! payload plus 6 bytes of zlib framing and 5 per stored block, with one
//! block even for empty input.

/// Errors returned by [`compress_dict_to`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CompressError {
    /// The caller-supplied output buffer was too small to hold the
    /// compressed dictionary. Size with [`compressed_size`].
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
/// Returns the number of bytes written into `output`. Size `output` with
/// [`compressed_size`].
///
/// # Errors
///
/// Returns [`CompressError::OutputTooSmall`] if `output` is not large
/// enough to hold the compressed stream.
pub fn compress_dict_to(input: &[u8], output: &mut [u8]) -> Result<usize, CompressError> {
    let needed = compressed_size(input.len());
    if output.len() < needed {
        return Err(CompressError::OutputTooSmall);
    }
    Ok(compress_dict_infallible(input, output))
}

/// Compress `input` into an exactly sized array at compile time.
///
/// The output is byte-for-byte identical to [`compress_dict_to`]. The
/// const generic `N` must equal [`compressed_size`] for `input`.
///
/// # Panics
///
/// Panics during const evaluation when `N` is not the exact compressed
/// stream length for `input`.
#[must_use]
pub const fn compress_dict<const N: usize>(input: &[u8]) -> [u8; N] {
    assert!(N == compressed_size(input.len()));
    let mut output = [0u8; N];
    let written = compress_dict_infallible(input, &mut output);
    assert!(written == N);
    output
}

const fn compress_dict_infallible(input: &[u8], output: &mut [u8]) -> usize {
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
        output[w] = 0x01;
        output[w + 1] = 0x00;
        output[w + 2] = 0x00;
        output[w + 3] = 0xFF;
        output[w + 4] = 0xFF;
        w += 5;
    } else {
        let mut read = 0usize;
        while read < input.len() {
            let remaining_len = input.len() - read;
            let chunk_len = if remaining_len < STORED_BLOCK_MAX {
                remaining_len
            } else {
                STORED_BLOCK_MAX
            };
            let is_last = chunk_len == remaining_len;
            #[allow(clippy::cast_possible_truncation)]
            let len_u16 = chunk_len as u16;
            output[w] = is_last as u8;
            w += 1;
            let len_bytes = len_u16.to_le_bytes();
            output[w] = len_bytes[0];
            output[w + 1] = len_bytes[1];
            w += 2;
            let inverted_len_bytes = (!len_u16).to_le_bytes();
            output[w] = inverted_len_bytes[0];
            output[w + 1] = inverted_len_bytes[1];
            w += 2;
            let mut copied = 0usize;
            while copied < chunk_len {
                output[w + copied] = input[read + copied];
                copied += 1;
            }
            w += chunk_len;
            read += chunk_len;
        }
    }

    // RFC 1950 trailer: Adler-32 of the uncompressed input, big-endian.
    let adler = adler32(input);
    let adler_bytes = adler.to_be_bytes();
    output[w] = adler_bytes[0];
    output[w + 1] = adler_bytes[1];
    output[w + 2] = adler_bytes[2];
    output[w + 3] = adler_bytes[3];
    w += 4;

    w
}

/// Exact length of the zlib stream the encoder emits for an input of
/// `uncompressed_len` bytes.
///
/// [`compress_dict`] requires its array length to equal this value, so
/// it must stay exact rather than a loose bound. Firmware sizes the static
/// compressed dictionary with it at compile time:
///
/// ```ignore
/// static COMPRESSED: [u8; ::ankyra::dictionary::compressed_size(DICT_BYTES.len())] =
///     ::ankyra::dictionary::compress_dict(DICT_BYTES);
/// ```
///
/// The stream consists of:
///
/// * 2 bytes for the zlib header.
/// * 5 bytes of framing per stored block, with one terminating block
///   even when `uncompressed_len == 0`.
/// * `uncompressed_len` bytes of literal payload.
/// * 4 bytes for the Adler-32 trailer.
#[must_use]
pub const fn compressed_size(uncompressed_len: usize) -> usize {
    let blocks = if uncompressed_len == 0 {
        1
    } else {
        uncompressed_len.div_ceil(STORED_BLOCK_MAX)
    };
    2 + 5 * blocks + uncompressed_len + 4
}

/// Adler-32 checksum per RFC 1950 §9.
const fn adler32(input: &[u8]) -> u32 {
    // `MOD_ADLER` is the largest prime below 65536 — RFC 1950 mandates
    // the Adler-32 reduction modulo this prime.
    const MOD_ADLER: u32 = 65521;
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    let mut index = 0usize;
    while index < input.len() {
        a = (a + input[index] as u32) % MOD_ADLER;
        b = (b + a) % MOD_ADLER;
        index += 1;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compressed_size_is_const_evaluable() {
        const BOUND_EMPTY: usize = compressed_size(0);
        const BOUND_SMALL: usize = compressed_size(1024);
        const BOUND_LARGE: usize = compressed_size(100_000);
        assert_eq!(BOUND_EMPTY, 2 + 5 + 4);
        assert_eq!(BOUND_SMALL, 2 + 5 + 1024 + 4);
        assert_eq!(BOUND_LARGE, 2 + 10 + 100_000 + 4);
    }

    /// Adler-32 test vector from RFC 1950: `adler32("")` = 1.
    #[test]
    fn adler32_empty_is_one() {
        assert_eq!(adler32(&[]), 1);
    }

    /// Adler-32 test vector: `adler32(b"Wikipedia")` = 0x11E60398.
    #[test]
    fn adler32_known_vector() {
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    /// Round-trip decoding needs `flate2` and lives in `tests/compression.rs`.
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
