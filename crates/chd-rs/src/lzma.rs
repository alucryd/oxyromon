//! Raw LZMA1 compression, the encoder side of the `lzma` codec.
//!
//! CHD chunks hold a raw LZMA1 stream: no container, no properties byte,
//! lc/lp/pb fixed at 3/0/2 and a dictionary derived from the hunk size.
//! MAME compresses hunks with the LZMA SDK's `LzmaEnc_MemEncode` in
//! non-final mode, so the stream carries no end-of-payload marker, and its
//! decoder rejects chunks that carry one. This module drives liblzma's raw
//! encoder with the `LZMA_FILTER_LZMA1EXT` filter and `ext_flags` left at 0,
//! which is that library's supported way to suppress the end marker.

use std::ffi::{c_int, c_void};
use std::mem::zeroed;

use liblzma_sys::{
    LZMA_FINISH, LZMA_OK, LZMA_STREAM_END, LZMA_VLI_UNKNOWN, lzma_filter, lzma_stream,
};

/// The LZMA1 filter variant that takes its options through the `ext_flags`
/// field of `lzma_options_lzma`, which controls the end-of-payload marker.
/// Not part of the generated bindings; `LZMA_FILTER_LZMA1EXT` in
/// `<lzma/lzma12.h>`.
const LZMA_FILTER_LZMA1EXT: u64 = 0x4000_0000_0000_0002;

/// A mirror of `lzma_options_lzma` from `<lzma/lzma12.h>` that names the
/// `ext_*` fields, which the bindings hide behind private reserved slots of
/// identical type. The `ext_*` fields reuse the old `reserved_int1..3`
/// positions, so the layout is unchanged.
#[repr(C)]
struct LzmaOptions {
    dict_size: u32,
    preset_dict: *const u8,
    preset_dict_size: u32,
    lc: u32,
    lp: u32,
    pb: u32,
    mode: c_int,
    nice_len: u32,
    mf: c_int,
    depth: u32,
    ext_flags: u32,
    ext_size_low: u32,
    ext_size_high: u32,
    reserved_int4: u32,
    reserved_int5: u32,
    reserved_int6: u32,
    reserved_int7: u32,
    reserved_int8: u32,
    reserved_enum1: c_int,
    reserved_enum2: c_int,
    reserved_enum3: c_int,
    reserved_enum4: c_int,
    reserved_ptr1: *mut c_void,
    reserved_ptr2: *mut c_void,
}

/// The encoder level MAME's `configure_properties` selects.
const PRESET: u32 = 6;
/// The LZMA dictionary bounds MAME's property normalization clamps to.
const DICT_SIZE_MIN: u32 = 4096;
const DICT_SIZE_MAX: u32 = 1 << 26;

/// Compress one hunk's worth of data into a raw LZMA1 stream.
///
/// The dictionary is clamped exactly the way MAME's `LzmaEncProps_Normalize`
/// clamps it for both its compressor and its decompressor, from the hunk
/// size, so a chunk produced here is always decodable by MAME. A chunk that
/// does not compress smaller than its source is reported as an error: MAME
/// treats it the same way, and its `find_best_compressor` moves on to the
/// next codec.
pub(crate) fn compress(source: &[u8], hunkbytes: usize) -> crate::Result<Vec<u8>> {
    let mut options: LzmaOptions = unsafe { zeroed() };
    let ok =
        unsafe { liblzma_sys::lzma_lzma_preset(std::ptr::from_mut(&mut options).cast(), PRESET) };
    if ok != 0 {
        return Err(crate::Error::Compression(
            "liblzma rejected the encoder preset".to_string(),
        ));
    }
    // MAME's property normalization fixes the coder parameters at level 6
    // defaults (lc/lp/pb 3/0/2, normal mode, BT4 matches, nice length 64)
    // and replaces the dictionary size with one derived from the hunk.
    options.lc = 3;
    options.lp = 0;
    options.pb = 2;
    options.dict_size = (hunkbytes as u32).clamp(DICT_SIZE_MIN, DICT_SIZE_MAX);

    let filters = [
        lzma_filter {
            id: LZMA_FILTER_LZMA1EXT,
            options: std::ptr::from_mut(&mut options).cast::<c_void>(),
        },
        lzma_filter {
            id: LZMA_VLI_UNKNOWN,
            options: std::ptr::null_mut(),
        },
    ];

    // The encoder never gets more output space than the source it was given:
    // a chunk that cannot shrink into it is rejected, like MAME's compressor
    // does.
    let mut output = vec![0u8; source.len()];
    let mut stream: lzma_stream = unsafe { zeroed() };
    if unsafe { liblzma_sys::lzma_raw_encoder(&raw mut stream, filters.as_ptr()) } != LZMA_OK {
        return Err(crate::Error::Compression(
            "liblzma failed to create the raw encoder".to_string(),
        ));
    }
    stream.next_in = source.as_ptr();
    stream.avail_in = source.len();
    stream.next_out = output.as_mut_ptr();
    stream.avail_out = output.len();

    let mut status = unsafe { liblzma_sys::lzma_code(&raw mut stream, LZMA_FINISH) };
    while status == LZMA_OK && stream.avail_out > 0 {
        status = unsafe { liblzma_sys::lzma_code(&raw mut stream, LZMA_FINISH) };
    }
    unsafe { liblzma_sys::lzma_end(&raw mut stream) };

    if status != LZMA_STREAM_END {
        return Err(crate::Error::Compression(format!(
            "liblzma failed to compress the chunk (status {status})"
        )));
    }
    output.truncate(stream.total_out as usize);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_struct_matches_liblzma() {
        assert_eq!(
            std::mem::size_of::<LzmaOptions>(),
            std::mem::size_of::<liblzma_sys::lzma_options_lzma>()
        );
        assert_eq!(
            std::mem::align_of::<LzmaOptions>(),
            std::mem::align_of::<liblzma_sys::lzma_options_lzma>()
        );
    }

    #[test]
    fn compress_round_trips_through_the_decoder() {
        // Highly compressible content, so the output is guaranteed to be
        // smaller than the input.
        let source: Vec<u8> = (0..2048u32).map(|i| (i % 251) as u8).collect();
        let compressed = compress(&source, 4096).unwrap();
        assert!(compressed.len() < source.len());

        let mut decompressed = vec![0u8; source.len()];
        crate::codec::Decompressor::decompress_lzma(
            &compressed,
            &mut decompressed,
            crate::codec::lzma_dict_size(4096),
        )
        .unwrap();
        assert_eq!(decompressed, source);
    }
}
