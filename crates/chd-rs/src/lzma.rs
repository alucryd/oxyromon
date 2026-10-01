//! Raw LZMA1 compression, the encoder side of the `lzma` codec.
//!
//! CHD chunks hold a raw LZMA1 stream: no container, no properties byte,
//! lc/lp/pb fixed at 3/0/2 and a dictionary derived from the hunk size.
//! MAME compresses hunks with its bundled LZMA SDK's `LzmaEnc_MemEncode`, at
//! level 8, without an end marker. `lzma-sdk-rs` ports that encoder (SDK
//! 23.01, the one MAME 0.289 bundles) byte for byte, where liblzma's encoder,
//! given the same parameters, makes slightly different choices.

/// Compress one hunk's worth of data into a raw LZMA1 stream.
///
/// `hunkbytes` sets the dictionary, as MAME's `configure_properties` does.
/// A chunk that does not compress smaller than its source is reported as an
/// error: MAME's encoder runs out of output space there, and its
/// `find_best_compressor` moves on to the next codec.
pub(crate) fn compress(source: &[u8], hunkbytes: usize) -> crate::Result<Vec<u8>> {
    let hunkbytes = u32::try_from(hunkbytes)
        .map_err(|_| crate::Error::Compression("hunk too large for LZMA".to_string()))?;
    let output = lzma_sdk_rs::encode(source, &lzma_sdk_rs::LzmaProps::chd_for_hunk(hunkbytes));
    if output.len() >= source.len() {
        return Err(crate::Error::Compression(
            "LZMA failed to compress the chunk".to_string(),
        ));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

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
