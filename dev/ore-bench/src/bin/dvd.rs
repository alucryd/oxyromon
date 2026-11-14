//! DVD measurements for the ORE study, PlayStation 2 first.
//!
//! `dvd data SCRATCH EXTENSIONS <CHD>...`: each CHD copied into SCRATCH and
//! extracted to its ISO, a DVD's or a CD's single Mode 1 track, its ISO 9660
//! file system mapped, then its sectors cut and compressed as an ORE would:
//! runs of zero sectors stored as their length, the rest MinCDC chunks,
//! over the whole disc or cut again at every file's start and end, deduped
//! within the disc and in a store holding every disc before it. A row a
//! disc on stdout; EXTENSIONS gets a row a disc and file extension.
//!
//! `dvd audio SCRATCH STREAMS <CHD>...`: each CHD extracted as above, a CD
//! of one Mode 2 track too, and the disc scanned at every 16 bytes for audio
//! headers: Sony's `SShd`, `VAGp` and `RIFF WAVE`. Each PCM stream through
//! Zstandard and FLAC, its samples as stored and de-interleaved; each
//! PS-ADPCM stream through Zstandard. A row a disc on stdout; STREAMS gets a
//! row a stream.
//!
//! The library is only ever read: everything written goes to SCRATCH, and
//! is removed once measured.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use cd::iso9660;
use lzma_rust2::{Lzma2Options, Lzma2Writer};
use mothcdc::mincdc::{MinCdcHash4, SliceChunker};
use rayon::prelude::*;
use sha1::{Digest, Sha1};

const SECTOR: u64 = 2048;
const MIB: usize = 1 << 20;
/// What is read at a time: a cut falls at every window's end. shortcut:
/// costs a chunk's worth of dedup a window; upgrade to a streaming chunker
/// if 64 MiB windows ever show in the numbers.
const WINDOW: u64 = 64 << 20;
/// The shortest run of zero sectors stored as its length, 32 KiB.
const ZERO_RUN: u64 = 16;
/// What a stored chunk and a reference to one cost in the index, as
/// ore-bench's other runs count them.
const REFERENCE: u64 = 8;
const STORED: u64 = 8;

#[derive(Clone, Copy)]
enum Codec {
    Zstd(usize),
    Lzma2(usize),
}

impl Codec {
    fn group(self) -> usize {
        match self {
            Self::Zstd(n) | Self::Lzma2(n) => n,
        }
    }

    fn compress(self, data: &[u8]) -> u64 {
        match self {
            Self::Zstd(window) => {
                let mut encoder = zstd::bulk::Compressor::new(19).unwrap();
                encoder
                    .set_parameter(zstd::zstd_safe::CParameter::WindowLog(window.ilog2()))
                    .unwrap();
                encoder
                    .set_parameter(zstd::zstd_safe::CParameter::ChecksumFlag(false))
                    .unwrap();
                encoder.compress(data).unwrap().len() as u64
            }
            Self::Lzma2(dict) => {
                let mut options = Lzma2Options::with_preset(9);
                options.lzma_options.dict_size = dict as u32;
                let mut writer = Lzma2Writer::new(Vec::new(), options);
                writer.write_all(data).unwrap();
                writer.finish().unwrap().len() as u64
            }
        }
    }
}

/// Groups filled in order and compressed, a batch at a time across every
/// core, so a disc never sits in memory whole.
struct Packer {
    codec: Codec,
    current: Vec<u8>,
    full: Vec<Vec<u8>>,
    compressed: u64,
    groups: u64,
}

impl Packer {
    fn new(codec: Codec) -> Self {
        Self {
            codec,
            current: Vec::new(),
            full: Vec::new(),
            compressed: 0,
            groups: 0,
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        if !self.current.is_empty() && self.current.len() + chunk.len() > self.codec.group() {
            self.full.push(std::mem::take(&mut self.current));
            if self.full.len() >= rayon::current_num_threads() * 2 {
                self.drain();
            }
        }
        self.current.extend_from_slice(chunk);
    }

    fn drain(&mut self) {
        let codec = self.codec;
        self.compressed += self
            .full
            .par_iter()
            .map(|group| codec.compress(group))
            .sum::<u64>();
        self.groups += self.full.len() as u64;
        self.full.clear();
    }

    fn finish(mut self) -> (u64, u64) {
        if !self.current.is_empty() {
            self.full.push(std::mem::take(&mut self.current));
        }
        self.drain();
        (self.compressed, self.groups)
    }
}

/// A way to cut a disc, its packers a codec each, alone and in the store.
struct Variant {
    name: &'static str,
    at_files: bool,
    codecs: Vec<Codec>,
}

#[derive(Default)]
struct Tally {
    chunks: u64,
    stored_chunks: u64,
    stored_bytes: u64,
}

/// `buf` without its runs of zero sectors of [`ZERO_RUN`] or more, which
/// `zeros` counts if given.
fn nonzero<'a>(buf: &'a [u8], mut zeros: Option<&mut u64>) -> Vec<&'a [u8]> {
    let sector = SECTOR as usize;
    let sectors = buf.len() / sector;
    let zero = |s: usize| buf[s * sector..(s + 1) * sector].iter().all(|&b| b == 0);
    let (mut pieces, mut from, mut s) = (Vec::new(), 0, 0);
    while s < sectors {
        if !zero(s) {
            s += 1;
            continue;
        }
        let mut e = s;
        while e < sectors && zero(e) {
            e += 1;
        }
        if (e - s) as u64 >= ZERO_RUN {
            if from < s * sector {
                pieces.push(&buf[from..s * sector]);
            }
            if let Some(zeros) = zeros.as_deref_mut() {
                *zeros += (e - s) as u64 * SECTOR;
            }
            from = e * sector;
        }
        s = e;
    }
    if from < buf.len() {
        pieces.push(&buf[from..]);
    }
    pieces
}

/// Every chunk of the disc within `bounds`, cut as ORE would, in order,
/// with its digest; `zeros` counts the zero runs if given.
fn for_each_chunk(
    file: &File,
    bounds: &[(u64, u64)],
    window: &mut [u8],
    mut zeros: Option<&mut u64>,
    f: &mut dyn FnMut(&[u8], [u8; 20]),
) -> Result<()> {
    for &(start, end) in bounds {
        let mut at = start;
        while at < end {
            let len = (end - at).min(WINDOW);
            let buf = &mut window[..len as usize];
            file.read_exact_at(buf, at)?;
            at += len;
            let pieces = nonzero(buf, zeros.as_deref_mut());
            let chunks: Vec<&[u8]> = pieces
                .iter()
                .flat_map(|piece| {
                    SliceChunker::new(piece, 8 << 10, 24 << 10, MinCdcHash4::new())
                        .map(|chunk| &piece[chunk.offset() as usize..][..chunk.len()])
                        .collect::<Vec<_>>()
                })
                .collect();
            let digests: Vec<[u8; 20]> = chunks
                .par_iter()
                .map(|chunk| Sha1::digest(chunk).into())
                .collect();
            for (chunk, digest) in chunks.iter().zip(digests) {
                f(chunk, digest);
            }
        }
    }
    Ok(())
}

/// The disc's areas, in order: each file's extent, and what lies between.
fn areas(entries: &[iso9660::Entry], size: u64) -> Vec<(u64, u64, Option<usize>)> {
    let mut files: Vec<(u64, u64, usize)> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| !entry.directory && entry.size > 0)
        .map(|(i, entry)| {
            let start = u64::from(entry.lba) * SECTOR;
            (start, (start + u64::from(entry.size)).min(size), i)
        })
        .filter(|&(start, end, _)| start < end)
        .collect();
    files.sort_unstable();
    let mut out = Vec::new();
    let mut at = 0;
    for (start, end, i) in files {
        // Files that overlap keep the first's extent
        if end <= at {
            continue;
        }
        let start = start.max(at);
        if start > at {
            out.push((at, start, None));
        }
        out.push((start, end, Some(i)));
        at = end;
    }
    if at < size {
        out.push((at, size, None));
    }
    out
}

fn extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    let name = name.split(';').next().unwrap_or(name);
    match name.rsplit_once('.') {
        Some((_, extension)) if !extension.is_empty() && extension.len() <= 4 => {
            extension.to_uppercase()
        }
        _ => "-".into(),
    }
}

/// Copy `chd` into `scratch` and extract its ISO there; a CD of one Mode 2
/// track too, as its sectors' first 2048 bytes of data, if `cooked`.
fn extract(chd: &Path, scratch: &Path, cooked: bool) -> Result<PathBuf> {
    let _ = fs::remove_dir_all(scratch);
    fs::create_dir_all(scratch)?;
    let local = scratch.join("disc.chd");
    fs::copy(chd, &local).with_context(|| format!("copying {}", chd.display()))?;
    // A clone's header opens without its parent, and names it
    let mut opened = match chd_rs::Chd::open(&local)?.info().parent_sha1 {
        None => chd_rs::Chd::open(&local)?,
        Some(sha1) => {
            let dir = chd.parent().unwrap_or(Path::new("."));
            let found = fs::read_dir(dir)?
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| {
                    path.extension()
                        .is_some_and(|x| x.eq_ignore_ascii_case("chd"))
                })
                .find(|path| chd_rs::Chd::open(path).is_ok_and(|chd| chd.info().sha1 == Some(sha1)))
                .with_context(|| format!("{}: its parent is not beside it", chd.display()))?;
            let local_parent = scratch.join("parent.chd");
            fs::copy(&found, &local_parent)?;
            chd_rs::Chd::open_with_parent(&local, &local_parent)?
        }
    };
    let iso = scratch.join("disc.iso");
    match opened.info().chd_type {
        chd_rs::ChdType::Dvd => opened.extract(&iso, &mut |_| {})?,
        chd_rs::ChdType::Cd => {
            // A DVD image some hacks ship as a CD of one Mode 1 track
            let files = chd_rs::extract_cd(
                &mut opened,
                &scratch.join("disc.cue"),
                Some(&iso),
                false,
                &mut |_| {},
            )?;
            let cue = fs::read_to_string(&files[0])?;
            if cue.matches("TRACK").count() != 1 {
                bail!("{}: a CD of several tracks, not an ISO", chd.display());
            }
            // A CD of one Mode 2 track: its Form 1 sectors' 2048 bytes
            if cue.contains("MODE2/2352") {
                if !cooked {
                    bail!("{}: a CD, not an ISO", chd.display());
                }
                let raw = scratch.join("disc.bin");
                fs::rename(&iso, &raw)?;
                let input = File::open(&raw)?;
                let mut output = std::io::BufWriter::new(File::create(&iso)?);
                let mut sector = vec![0; 2352];
                let sectors = input.metadata()?.len() / 2352;
                for i in 0..sectors {
                    input.read_exact_at(&mut sector, i * 2352)?;
                    output.write_all(&sector[24..24 + 2048])?;
                }
                output.flush()?;
                fs::remove_file(&raw)?;
            } else if !cue.contains("MODE1/2048") {
                bail!("{}: a CD, not an ISO", chd.display());
            }
        }
        other => bail!("{}: a {other:?} CHD", chd.display()),
    }
    drop(opened);
    for name in ["disc.chd", "parent.chd"] {
        let _ = fs::remove_file(scratch.join(name));
    }
    Ok(iso)
}

fn data(scratch: &Path, extensions: &Path, chds: &[PathBuf]) -> Result<()> {
    let variants = [
        Variant {
            name: "whole",
            at_files: false,
            codecs: vec![Codec::Zstd(MIB)],
        },
        Variant {
            name: "files",
            at_files: true,
            codecs: vec![
                Codec::Zstd(MIB),
                Codec::Zstd(4 * MIB),
                Codec::Lzma2(4 * MIB),
            ],
        },
    ];
    let mut seen: Vec<HashSet<[u8; 20]>> = variants.iter().map(|_| HashSet::new()).collect();
    let mut extensions = File::create(extensions)?;
    writeln!(extensions, "disc\textension\tfiles\tbytes")?;
    print!("disc\tchd\tiso\tfiles\tfile_bytes\tgap_bytes\tzero_bytes\tseconds");
    for variant in &variants {
        for scope in ["disc", "store"] {
            print!(
                "\t{0}_{1}_chunks\t{0}_{1}_stored_chunks\t{0}_{1}_stored_bytes",
                variant.name, scope
            );
            for codec in &variant.codecs {
                let codec = match codec {
                    Codec::Zstd(n) => format!("zstd{}", n / MIB),
                    Codec::Lzma2(n) => format!("lzma{}", n / MIB),
                };
                print!("\t{}_{scope}_{codec}", variant.name);
            }
        }
    }
    println!();

    for chd in chds {
        let start = Instant::now();
        let name = chd.file_name().unwrap().to_string_lossy().into_owned();
        eprintln!("{name}");
        let iso = extract(chd, scratch, false)?;
        let file = File::open(&iso)?;
        let size = file.metadata()?.len();
        let entries =
            iso9660::entries(&mut |lba, buf| file.read_exact_at(buf, u64::from(lba) * SECTOR))?;
        let areas = areas(&entries, size);

        let mut by_extension: HashMap<String, (u64, u64)> = HashMap::new();
        let (mut files, mut file_bytes, mut gap_bytes, mut zero_bytes) = (0, 0, 0, 0);
        for &(start, end, entry) in &areas {
            match entry {
                Some(i) => {
                    files += 1;
                    file_bytes += end - start;
                    let tally = by_extension.entry(extension(&entries[i].path)).or_default();
                    tally.0 += 1;
                    tally.1 += end - start;
                }
                None => gap_bytes += end - start,
            }
        }

        // Each variant's chunks: the disc's own, and those the store lacks.
        // The store's are compressed in a second pass, and only for a disc
        // sharing chunks with one before it: otherwise they are the disc's.
        print!(
            "{name}\t{}\t{size}\t{files}\t{file_bytes}\t{gap_bytes}",
            fs::metadata(chd)?.len()
        );
        let mut window = vec![0; WINDOW as usize];
        let mut row = String::new();
        for (v, variant) in variants.iter().enumerate() {
            let bounds: Vec<(u64, u64)> = if variant.at_files {
                areas.iter().map(|&(start, end, _)| (start, end)).collect()
            } else {
                vec![(0, size)]
            };
            let mut tallies: [Tally; 2] = Default::default();
            let mut packers: Vec<Packer> = variant.codecs.iter().map(|&c| Packer::new(c)).collect();
            let mut own = HashSet::new();
            let mut fresh = HashSet::new();
            let zeros = (v == 0).then_some(&mut zero_bytes);
            for_each_chunk(&file, &bounds, &mut window, zeros, &mut |chunk, digest| {
                tallies[0].chunks += 1;
                tallies[1].chunks += 1;
                if own.insert(digest) {
                    tallies[0].stored_chunks += 1;
                    tallies[0].stored_bytes += chunk.len() as u64;
                    packers.iter_mut().for_each(|p| p.push(chunk));
                    if seen[v].insert(digest) {
                        tallies[1].stored_chunks += 1;
                        tallies[1].stored_bytes += chunk.len() as u64;
                        fresh.insert(digest);
                    }
                }
            })?;
            let disc: Vec<(u64, u64)> = packers.into_iter().map(Packer::finish).collect();
            let store = if fresh.len() as u64 == tallies[0].stored_chunks {
                disc.clone()
            } else {
                let mut packers: Vec<Packer> =
                    variant.codecs.iter().map(|&c| Packer::new(c)).collect();
                for_each_chunk(&file, &bounds, &mut window, None, &mut |chunk, digest| {
                    if fresh.remove(&digest) {
                        packers.iter_mut().for_each(|p| p.push(chunk));
                    }
                })?;
                packers.into_iter().map(Packer::finish).collect()
            };
            for (tally, sizes) in tallies.iter().zip([disc, store]) {
                row += &format!(
                    "\t{}\t{}\t{}",
                    tally.chunks, tally.stored_chunks, tally.stored_bytes
                );
                for (compressed, groups) in sizes {
                    let index =
                        tally.chunks * REFERENCE + tally.stored_chunks * STORED + groups * 16;
                    row += &format!("\t{}", compressed + index);
                }
            }
        }
        println!("\t{zero_bytes}\t{:.0}{row}", start.elapsed().as_secs_f64());
        std::io::stdout().flush()?;

        let mut rows: Vec<_> = by_extension.into_iter().collect();
        rows.sort_by_key(|row| std::cmp::Reverse(row.1.1));
        for (extension, (count, bytes)) in rows {
            writeln!(extensions, "{name}\t{extension}\t{count}\t{bytes}")?;
        }
        fs::remove_dir_all(scratch)?;
    }
    Ok(())
}

/// An audio stream found on a disc: where its samples lie, and how.
struct Stream {
    container: &'static str,
    /// 16-bit PCM rather than PS-ADPCM.
    pcm: bool,
    at: u64,
    len: u64,
    rate: u32,
    channels: u32,
    /// Bytes of one channel before the next's, 0 when samples interleave.
    interleave: u32,
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(b[at..at + 4].try_into().unwrap())
}

/// The stream whose header starts `b`, at `at` on a disc of `size` bytes,
/// if its header is plausible.
fn stream(b: &[u8], at: u64, size: u64) -> Option<Stream> {
    let rate_ok = |rate: u32| (4000..=96000).contains(&rate);
    let fits = |start: u64, len: u64| len > 0 && start + len <= size;
    match &b[..4] {
        // Sony's ADS: SShd, then SSbd and its size
        b"SShd" if le32(b, 4) == 0x18 && &b[0x20..0x24] == b"SSbd" => {
            let (format, rate, channels, interleave) =
                (le32(b, 8), le32(b, 12), le32(b, 16), le32(b, 20));
            let pcm = match format {
                1 => true,
                0x10 => false,
                _ => return None,
            };
            let len = u64::from(le32(b, 0x24));
            let start = at + 0x28;
            (rate_ok(rate) && (1..=8).contains(&channels) && fits(start, len)).then_some(Stream {
                container: "SShd",
                pcm,
                at: start,
                len,
                rate,
                channels,
                interleave: if channels > 1 { interleave } else { 0 },
            })
        }
        // A VAG: mono PS-ADPCM, big endian, its data after 48 bytes
        b"VAGp" => {
            let (len, rate) = (u64::from(be32(b, 0x0c)), be32(b, 0x10));
            let start = at + 0x30;
            (rate_ok(rate) && len % 16 == 0 && fits(start, len)).then_some(Stream {
                container: "VAGp",
                pcm: false,
                at: start,
                len,
                rate,
                channels: 1,
                interleave: 0,
            })
        }
        // A WAVE of 16-bit PCM, its fmt chunk first, then its data
        b"RIFF" if &b[8..16] == b"WAVEfmt " => {
            let fmt = le32(b, 16) as usize;
            let (format, channels, rate, bits) = (
                u16::from_le_bytes([b[20], b[21]]),
                u32::from(u16::from_le_bytes([b[22], b[23]])),
                le32(b, 24),
                u16::from_le_bytes([b[34], b[35]]),
            );
            let data = 20 + fmt;
            if format != 1 || bits != 16 || data + 8 > b.len() || &b[data..data + 4] != b"data" {
                return None;
            }
            let len = u64::from(le32(b, data + 4));
            let start = at + data as u64 + 8;
            (rate_ok(rate) && (1..=8).contains(&channels) && fits(start, len)).then_some(Stream {
                container: "RIFF",
                pcm: true,
                at: start,
                len,
                rate,
                channels,
                interleave: 0,
            })
        }
        _ => None,
    }
}

/// Every stream on the disc, a header looked for at every 16 bytes, the
/// scan skipping past each stream found.
fn streams(file: &File, size: u64) -> Result<Vec<Stream>> {
    let mut found = Vec::new();
    let mut window = vec![0; WINDOW as usize + 4096];
    let mut at = 0;
    while at < size {
        let len = (size - at).min(window.len() as u64) as usize;
        file.read_exact_at(&mut window[..len], at)?;
        let mut next = at + WINDOW.min(size - at);
        let mut i = 0;
        // A header read whole only before the window's last 4 KiB
        while i + 64 <= len && (i as u64) < WINDOW {
            if let Some(found_stream) = stream(&window[i..len], at + i as u64, size) {
                let end = found_stream.at + found_stream.len;
                found.push(found_stream);
                if end > at + WINDOW {
                    next = end.next_multiple_of(16);
                    break;
                }
                i = (end - at).next_multiple_of(16) as usize;
                continue;
            }
            i += 16;
        }
        at = next;
    }
    Ok(found)
}

/// FLAC -8 of 16-bit samples, `channels` of them a frame, as `flac` writes.
fn flac(samples: &[u8], channels: u32, rate: u32) -> Result<u64> {
    let mut child = std::process::Command::new("flac")
        .args([
            "-8",
            "-s",
            "-c",
            "--force-raw-format",
            "--endian=little",
            "--sign=signed",
            "--bps=16",
        ])
        .arg(format!("--channels={channels}"))
        .arg(format!("--sample-rate={rate}"))
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("running flac")?;
    let mut stdin = child.stdin.take().unwrap();
    let even = samples.len() / (2 * channels as usize) * (2 * channels as usize);
    let samples = samples[..even].to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&samples));
    let output = child.wait_with_output()?;
    writer.join().unwrap()?;
    if !output.status.success() {
        bail!("flac failed");
    }
    Ok(output.stdout.len() as u64)
}

/// Interleaved blocks of `interleave` bytes a channel as frames of samples.
fn deinterleave(data: &[u8], channels: usize, interleave: usize) -> Vec<u8> {
    let round = channels * interleave;
    let rounds = data.len() / round;
    let mut out = Vec::with_capacity(rounds * round);
    for r in 0..rounds {
        let base = r * round;
        for sample in (0..interleave).step_by(2) {
            for channel in 0..channels {
                let at = base + channel * interleave + sample;
                out.extend_from_slice(&data[at..at + 2]);
            }
        }
    }
    out
}

fn audio(scratch: &Path, streams_tsv: &Path, chds: &[PathBuf]) -> Result<()> {
    let mut rows = File::create(streams_tsv)?;
    writeln!(
        rows,
        "disc	container	pcm	offset	bytes	rate	channels	interleave	zstd	flac	flac_deinterleaved"
    )?;
    println!(
        "disc	iso	pcm_streams	pcm_bytes	pcm_zstd	pcm_flac	pcm_flac_deinterleaved	adpcm_streams	adpcm_bytes	adpcm_zstd	seconds"
    );
    for chd in chds {
        let start = Instant::now();
        let name = chd.file_name().unwrap().to_string_lossy().into_owned();
        eprintln!("{name}");
        let iso = extract(chd, scratch, true)?;
        let file = File::open(&iso)?;
        let size = file.metadata()?.len();
        let found = streams(&file, size)?;
        let measured: Vec<(u64, u64, u64)> = found
            .par_iter()
            .map(|stream| -> Result<(u64, u64, u64)> {
                let mut data = vec![0; stream.len as usize];
                file.read_exact_at(&mut data, stream.at)?;
                // Zstandard in 1 MiB groups, as an ORE stores it now
                let zstd: u64 = data.chunks(MIB).map(|g| Codec::Zstd(MIB).compress(g)).sum();
                if !stream.pcm || stream.channels > 8 {
                    return Ok((zstd, 0, 0));
                }
                let flac_as_is = flac(&data, stream.channels, stream.rate)?;
                let flac_de = if stream.interleave > 1 {
                    flac(
                        &deinterleave(&data, stream.channels as usize, stream.interleave as usize),
                        stream.channels,
                        stream.rate,
                    )?
                } else {
                    flac_as_is
                };
                Ok((zstd, flac_as_is, flac_de))
            })
            .collect::<Result<_>>()?;
        let mut totals = [0u64; 8];
        for (stream, &(zstd, flac_as_is, flac_de)) in found.iter().zip(&measured) {
            writeln!(
                rows,
                "{name}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{zstd}\t{flac_as_is}\t{flac_de}",
                stream.container,
                stream.pcm,
                stream.at,
                stream.len,
                stream.rate,
                stream.channels,
                stream.interleave
            )?;
            if stream.pcm {
                totals[0] += 1;
                totals[1] += stream.len;
                totals[2] += zstd;
                totals[3] += flac_as_is;
                totals[4] += flac_de;
            } else {
                totals[5] += 1;
                totals[6] += stream.len;
                totals[7] += zstd;
            }
        }
        let t = totals;
        println!(
            "{name}\t{size}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.0}",
            t[0],
            t[1],
            t[2],
            t[3],
            t[4],
            t[5],
            t[6],
            t[7],
            start.elapsed().as_secs_f64()
        );
        std::io::stdout().flush()?;
        fs::remove_dir_all(scratch)?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("data") if args.len() > 3 => data(
            Path::new(&args[1]),
            Path::new(&args[2]),
            &args[3..].iter().map(PathBuf::from).collect::<Vec<_>>(),
        ),
        Some("audio") if args.len() > 3 => audio(
            Path::new(&args[1]),
            Path::new(&args[2]),
            &args[3..].iter().map(PathBuf::from).collect::<Vec<_>>(),
        ),
        _ => bail!("usage: dvd data|audio SCRATCH EXTENSIONS|STREAMS <CHD>..."),
    }
}
