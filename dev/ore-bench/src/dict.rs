//! Dictionaries (B3): Zstandard dictionaries trained on a library's chunks,
//! to give small units the context a long window gives a solid stream while
//! each stays readable alone.
//!
//! In three passes, as a disc at a time is all the local disk holds:
//! [`sample`] takes about 32 MB of every disc's 16 KiB FastCDC chunks,
//! [`train`] trains dictionaries on them, and [`units`] compresses every
//! chunk with each, as rows [`super::dedup::sum`] adds up. Dictionaries are
//! trained on:
//!
//! - `self`: the disc's own samples, as a dictionary stored with each disc;
//! - `others`: every other disc's samples, never the disc's own, as a shared
//!   dictionary meets a disc it was not trained on;
//! - `all`: every disc's samples, as a library's dictionary is trained on
//!   the library it compresses.
//!
//! A dictionary's own size is counted, as a row of its own: once per disc
//! for `self` and `others`, which differ by disc, once for `all`.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use anyhow::{Context, Result};
use fastcdc::v2020::FastCDC;
use rayon::prelude::*;
use sha1::{Digest, Sha1};
use zstd::bulk::Compressor;
use zstd::dict::EncoderDictionary;

use super::dedup::for_each_area;

const LEVEL: i32 = 19;
const AVERAGE: usize = 16 << 10;
/// Samples taken from each disc, in bytes.
const SAMPLE_BUDGET: u64 = 32 << 20;

/// The dictionaries trained, by source and size.
pub const SIZES: &[(&str, usize)] = &[("110K", 110 << 10), ("1M", 1 << 20)];
/// The one size only trained on every disc's samples, as fewer samples than
/// a hundred times the dictionary train it poorly.
pub const ALL_ONLY: (&str, usize) = ("8M", 8 << 20);

fn chunks(path: &Path, f: &mut dyn FnMut(&[u8]) -> Result<()>) -> Result<()> {
    for_each_area(path, &mut |area| {
        for chunk in FastCDC::new(&area, AVERAGE / 4, AVERAGE, AVERAGE * 4) {
            f(&area[chunk.offset..chunk.offset + chunk.length])?;
        }
        Ok(())
    })
}

/// Take about [`SAMPLE_BUDGET`] of a disc's chunks, evenly spread, into
/// `output`, each as its length then its bytes. Returns how many and their
/// bytes.
pub fn sample(disc: &Path, output: &Path) -> Result<(u64, u64)> {
    // Every chunk's length first, to spread the budget over the whole disc
    let mut total = 0u64;
    chunks(disc, &mut |chunk| {
        total += chunk.len() as u64;
        Ok(())
    })?;
    let every = total.div_ceil(SAMPLE_BUDGET).max(1);
    let mut out = std::io::BufWriter::new(fs::File::create(output)?);
    let (mut index, mut count, mut bytes) = (0u64, 0u64, 0u64);
    chunks(disc, &mut |chunk| {
        if index % every == 0 {
            out.write_all(&(chunk.len() as u32).to_le_bytes())?;
            out.write_all(chunk)?;
            count += 1;
            bytes += chunk.len() as u64;
        }
        index += 1;
        Ok(())
    })?;
    out.flush()?;
    Ok((count, bytes))
}

pub fn read_samples(path: &Path) -> Result<Vec<Vec<u8>>> {
    let mut data = Vec::new();
    fs::File::open(path)?.read_to_end(&mut data)?;
    let mut samples = Vec::new();
    let mut at = 0;
    while at < data.len() {
        let length = u32::from_le_bytes(data[at..at + 4].try_into()?) as usize;
        samples.push(data[at + 4..at + 4 + length].to_vec());
        at += 4 + length;
    }
    Ok(samples)
}

fn train_one(samples: &[&Vec<u8>], size: usize, output: &Path) -> Result<()> {
    let dictionary = zstd::dict::from_samples(samples, size)
        .with_context(|| format!("Failed to train \"{}\"", output.display()))?;
    fs::write(output, dictionary)?;
    Ok(())
}

/// Train every dictionary from the `<disc>.samples` files in `samples` into
/// `dictionaries`, as `all-<size>.dict`, `self-<size>-<disc>.dict` and
/// `others-<size>-<disc>.dict`.
pub fn train(samples: &Path, dictionaries: &Path) -> Result<()> {
    fs::create_dir_all(dictionaries)?;
    let mut by_disc: BTreeMap<String, Vec<Vec<u8>>> = BTreeMap::new();
    for entry in fs::read_dir(samples)? {
        let path = entry?.path();
        if let Some(disc) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".samples"))
        {
            by_disc.insert(disc.to_string(), read_samples(&path)?);
        }
    }
    let mut jobs: Vec<(Vec<&Vec<u8>>, usize, std::path::PathBuf)> = Vec::new();
    let all: Vec<&Vec<u8>> = by_disc.values().flatten().collect();
    for &(name, size) in SIZES.iter().chain([&ALL_ONLY]) {
        jobs.push((
            all.clone(),
            size,
            dictionaries.join(format!("all-{name}.dict")),
        ));
    }
    for disc in by_disc.keys() {
        for &(name, size) in SIZES {
            jobs.push((
                by_disc[disc].iter().collect(),
                size,
                dictionaries.join(format!("self-{name}-{disc}.dict")),
            ));
            let others: Vec<&Vec<u8>> = by_disc
                .iter()
                .filter(|(other, _)| *other != disc)
                .flat_map(|(_, samples)| samples)
                .collect();
            jobs.push((
                others,
                size,
                dictionaries.join(format!("others-{name}-{disc}.dict")),
            ));
        }
    }
    jobs.into_par_iter()
        .try_for_each(|(samples, size, output)| {
            eprintln!("training {}", output.display());
            train_one(&samples, size, &output)
        })
}

fn digest(data: &[u8]) -> Result<u64> {
    Ok(u64::from_be_bytes(Sha1::digest(data)[..8].try_into()?))
}

/// Print a row per 16 KiB chunk of `disc` and per dictionary from
/// `dictionaries`, as `dedup units` prints them, named after the disc.
pub fn units(dictionaries: &Path, disc: &Path, name: &str) -> Result<()> {
    // The dictionaries this disc is compressed with, by variant
    let mut variants: Vec<(String, Vec<u8>)> = Vec::new();
    for &(size, _) in SIZES {
        for source in ["self", "others"] {
            let path = dictionaries.join(format!("{source}-{size}-{name}.dict"));
            variants.push((
                format!("{source}-{size}"),
                fs::read(&path)
                    .with_context(|| format!("Failed to read \"{}\"", path.display()))?,
            ));
        }
    }
    for &(size, _) in SIZES.iter().chain([&ALL_ONLY]) {
        let path = dictionaries.join(format!("all-{size}.dict"));
        variants.push((
            format!("all-{size}"),
            fs::read(&path).with_context(|| format!("Failed to read \"{}\"", path.display()))?,
        ));
    }
    let prepared: Vec<EncoderDictionary<'static>> = variants
        .iter()
        .map(|(_, dictionary)| EncoderDictionary::copy(dictionary, LEVEL))
        .collect();

    // Each dictionary is stored once, as a unit of its own
    for (variant, dictionary) in &variants {
        println!(
            "{name}\tcdc-16K/dict-{variant}\t{:016x}\t0\t{}",
            digest(dictionary)?,
            dictionary.len()
        );
    }

    let mut pieces: Vec<Vec<u8>> = Vec::new();
    let flush = |pieces: &mut Vec<Vec<u8>>| -> Result<()> {
        let rows: Vec<String> = pieces
            .par_iter()
            .map_init(
                || {
                    let plain = Compressor::new(LEVEL).expect("a Zstandard compressor");
                    let with: Vec<Compressor<'_>> = prepared
                        .iter()
                        .map(|dictionary| {
                            Compressor::with_prepared_dictionary(dictionary)
                                .expect("a Zstandard compressor")
                        })
                        .collect();
                    (plain, with)
                },
                |(plain, with), chunk| -> Result<String> {
                    let digest = digest(chunk)?;
                    let length = chunk.len();
                    let mut rows = format!(
                        "{name}\tcdc-16K\t{digest:016x}\t{length}\t{}\n",
                        plain.compress(chunk)?.len().min(length)
                    );
                    for ((variant, _), compressor) in variants.iter().zip(with.iter_mut()) {
                        let size = compressor.compress(chunk)?.len().min(length);
                        rows += &format!(
                            "{name}\tcdc-16K/dict-{variant}\t{digest:016x}\t{length}\t{size}\n"
                        );
                    }
                    Ok(rows)
                },
            )
            .collect::<Result<_>>()?;
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        for row in rows {
            out.write_all(row.as_bytes())?;
        }
        pieces.clear();
        Ok(())
    };
    chunks(disc, &mut |chunk| {
        pieces.push(chunk.to_vec());
        if pieces.len() == 4096 {
            flush(&mut pieces)?;
        }
        Ok(())
    })?;
    flush(&mut pieces)
}
