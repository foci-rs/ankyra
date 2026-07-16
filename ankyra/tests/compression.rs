//! Integration tests for [`ankyra::dictionary::compress_dict_to`].
//!
//! These live outside the lib crate because the round-trip assertion
//! uses `flate2::read::ZlibDecoder`, which needs `std`. Integration
//! tests always have `std` available regardless of whether the
//! crate-under-test is `no_std`, which avoids the need to gate the
//! `compress_dict_to` tests behind the `std` feature when they would
//! still be perfectly exercising the `no_std` code path.

use std::io::Read;

use ankyra::dictionary::{CompressError, compress_dict_to, compressed_size};

/// Round-trip a sample payload through `compress_dict_to` and
/// `flate2::read::ZlibDecoder`. Confirms the output is proper RFC 1950
/// zlib (not raw deflate) and byte-for-byte equal to the input.
#[test]
fn round_trip_produces_zlib_framed_stream() {
    let input = br#"{"commands":{"identify offset=%u count=%u":1}}"#;
    let mut scratch = [0u8; 512];
    let n = compress_dict_to(input, &mut scratch).expect("compression fits in scratch");
    let mut decoder = flate2::read::ZlibDecoder::new(&scratch[..n]);
    let mut round = Vec::new();
    decoder.read_to_end(&mut round).expect("zlib decode");
    assert_eq!(round.as_slice(), input);
}

/// An empty input still produces a valid zlib stream — the encoder
/// emits one terminating stored block plus the Adler-32 trailer.
#[test]
fn empty_input_round_trips() {
    let mut scratch = [0u8; 32];
    let n = compress_dict_to(&[], &mut scratch).expect("empty compression fits");
    assert!(n > 0);
    let mut decoder = flate2::read::ZlibDecoder::new(&scratch[..n]);
    let mut round = Vec::new();
    decoder.read_to_end(&mut round).expect("zlib decode");
    assert!(round.is_empty());
}

/// Inputs that span multiple 64 KiB stored blocks round-trip
/// intact — confirms the chunk-splitting path.
#[test]
#[allow(clippy::cast_possible_truncation)]
fn multi_block_input_round_trips() {
    let input: Vec<u8> = (0..200_000u32).map(|i| (i & 0xFF) as u8).collect();
    let mut scratch = vec![0u8; compressed_size(input.len())];
    let n = compress_dict_to(&input, &mut scratch).expect("multi-block compression fits");
    let mut decoder = flate2::read::ZlibDecoder::new(&scratch[..n]);
    let mut round = Vec::new();
    decoder.read_to_end(&mut round).expect("zlib decode");
    assert_eq!(round, input);
}

/// Bound holds even for input sizes that cross the stored-block
/// ceiling: the reserved buffer is always enough.
#[test]
fn bound_is_tight_enough_for_chunked_input() {
    let input = vec![0x42u8; 123_456];
    let mut scratch = vec![0u8; compressed_size(input.len())];
    let n = compress_dict_to(&input, &mut scratch).expect("fits");
    assert!(n <= scratch.len());
}

/// When the output buffer is too small, the helper returns
/// `OutputTooSmall` instead of partially writing.
#[test]
fn oversize_input_rejects_tiny_output() {
    let input = [0x11u8; 256];
    let mut tiny = [0u8; 16];
    assert_eq!(
        compress_dict_to(&input, &mut tiny),
        Err(CompressError::OutputTooSmall)
    );
}
