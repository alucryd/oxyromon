//! Several discs as one solid stream: what a long window finds across discs,
//! alone and after dedup, against each disc as a solid stream of its own.
//!
//! Every stream is Zstandard level 19 with a 2 GiB window, the most it
//! takes, and long-distance matching: a match can only reach back 2 GiB, so a
//! disc further away than that is out of reach. It is made in 256 MiB jobs
//! on four threads, each seeing 256 MiB before its own job: at Zstandard's
//! default, a whole window a job on every core, the buffers alone outgrow
//! this machine's memory on a Wii disc. Long-distance matches are found over
//! the whole window all the same. Discs are read as [`for_each_area`] reads them.

use std::collections::HashSet;
use std::io::Write;
use std::path::Path;

use anyhow::Result;
use fastcdc::v2020::FastCDC;
use sha1::{Digest, Sha1};
use zstd::stream::Encoder;
use zstd::zstd_safe::CParameter;

use super::dedup::{REFERENCE, STORED, for_each_area};

/// A sink that only counts what it is written.
#[derive(Default)]
pub struct Count(pub u64);

impl Write for Count {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn encoder() -> Result<Encoder<'static, Count>> {
    let mut encoder = Encoder::new(Count::default(), 19)?;
    encoder.window_log(31)?;
    encoder.long_distance_matching(true)?;
    encoder.multithread(4)?;
    encoder.set_parameter(CParameter::JobSize(256 << 20))?;
    // A window is 2^9 overlaps: 6 leaves each job an eighth of it
    encoder.set_parameter(CParameter::OverlapSizeLog(6))?;
    Ok(encoder)
}

/// Each disc as a solid stream of its own, in bytes.
pub fn alone(discs: &[&Path]) -> Result<Vec<u64>> {
    discs
        .iter()
        .map(|disc| {
            let mut encoder = encoder()?;
            for_each_area(disc, &mut |area| Ok(encoder.write_all(&area)?))?;
            Ok(encoder.finish()?.0)
        })
        .collect()
}

/// The discs as one solid stream, in order, in bytes.
pub fn together(discs: &[&Path]) -> Result<u64> {
    let mut encoder = encoder()?;
    for disc in discs {
        for_each_area(disc, &mut |area| Ok(encoder.write_all(&area)?))?;
    }
    Ok(encoder.finish()?.0)
}

/// The discs deduped into `average`-byte FastCDC chunks, then what is stored
/// as one solid stream, in order: its bytes, index included, and the chunks
/// referenced and stored.
pub fn deduped(discs: &[&Path], average: usize) -> Result<(u64, u64, u64)> {
    let mut encoder = encoder()?;
    let mut seen: HashSet<[u8; 20]> = HashSet::new();
    let (mut references, mut stored) = (0u64, 0u64);
    for disc in discs {
        for_each_area(disc, &mut |area| {
            for chunk in FastCDC::new(&area, average / 4, average, average * 4) {
                let bytes = &area[chunk.offset..chunk.offset + chunk.length];
                references += 1;
                if seen.insert(Sha1::digest(bytes).into()) {
                    stored += 1;
                    encoder.write_all(bytes)?;
                }
            }
            Ok(())
        })?;
    }
    let size = encoder.finish()?.0;
    Ok((
        size + references * REFERENCE + stored * STORED,
        references,
        stored,
    ))
}
