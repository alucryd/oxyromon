//! Raw LZMA1, the `lzma` codec and the sector data of `cdlz`.
//!
//! CHD chunks hold a raw LZMA1 stream: no container, no properties byte,
//! lc/lp/pb fixed at 3/0/2, a dictionary derived from the hunk size, and no
//! end marker. Both directions go through `lzma-rust2`, whose encoder is set
//! the way MAME sets its LZMA SDK one (level 8: normal mode, BT4 matches,
//! nice length 64). The streams decode in MAME and chdman, though not byte for
//! byte as theirs: the two encoders choose matches a little differently, to
//! within a few hundredths of a percent of the same size.

use std::io::{Read, Write};

use crate::codec::lzma_dict_size;
use crate::{Error, Result};

const LC: u32 = 3;
const LP: u32 = 0;
const PB: u32 = 2;

/// Compress one hunk's worth of data into a raw LZMA1 stream.
///
/// `hunkbytes` sets the dictionary, as MAME's `configure_properties` does,
/// so every match lies within what MAME's decoder keeps. A chunk that does
/// not compress smaller than its source is reported as an error: MAME's
/// encoder runs out of output space there, and its `find_best_compressor`
/// moves on to the next codec.
pub(crate) fn compress(source: &[u8], hunkbytes: usize) -> Result<Vec<u8>> {
    let hunkbytes = u32::try_from(hunkbytes)
        .map_err(|_| Error::Compression("hunk too large for LZMA".to_string()))?;
    let mut options = lzma_rust2::LzmaOptions::with_preset(6);
    options.dict_size = lzma_dict_size(hunkbytes);
    options.lc = LC;
    options.lp = LP;
    options.pb = PB;
    let failed = |_| Error::Compression("LZMA failed to compress the chunk".to_string());
    let mut writer =
        lzma_rust2::LzmaWriter::new_no_header(Vec::with_capacity(source.len()), &options, false)
            .map_err(failed)?;
    writer.write_all(source).map_err(failed)?;
    let output = writer.finish().map_err(failed)?;
    if output.len() >= source.len() {
        return Err(Error::Compression(
            "LZMA failed to compress the chunk".to_string(),
        ));
    }
    Ok(output)
}

/// Decompress a raw LZMA1 chunk into `dest`, which it must fill exactly.
pub(crate) fn decompress(source: &[u8], dest: &mut [u8], dict_size: u32) -> Result<()> {
    let corrupt = |_| Error::Corrupt("lzma chunk is corrupt".to_string());
    lzma_rust2::LzmaReader::new(source, dest.len() as u64, LC, LP, PB, dict_size, None)
        .map_err(corrupt)?
        .read_exact(dest)
        .map_err(corrupt)
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
        decompress(&compressed, &mut decompressed, lzma_dict_size(4096)).unwrap();
        assert_eq!(decompressed, source);
    }
}
