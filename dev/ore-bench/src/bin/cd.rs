//! CD measurements for the ORE study, PlayStation first.
//!
//! `cd data SCRATCH <CHD>...`: each CHD copied into SCRATCH, extracted, its
//! data sectors checked to rebuild byte for byte from the `cd` crate's
//! model, its ISO 9660 file system mapped, and its data compressed as an
//! ORE would: raw sectors in groups, then planes, deduped within the disc
//! and in a store holding every disc before it. A row a disc.
//!
//! `cd audio SCRATCH <CHD>...`: every audio track through each lossless
//! codec, size and seconds to encode and decode. A row a track and codec.
//!
//! `cd sbi <SBI>...`: each SBI's Q-channel records against the Q a sector
//! at that address has. A row an entry.
//!
//! The library is only ever read: everything written goes to SCRATCH, and
//! is removed once measured.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use cd::cue::{self, Mode};
use cd::iso9660::{self, XA_FORM2, XA_INTERLEAVED};
use cd::sector::{self, FORM1_DATA, Kind, LEAD_IN, SECTOR};
use lzma_rust2::{Lzma2Options, Lzma2Writer};
use mothcdc::mincdc::{MinCdcHash4, SliceChunker};
use rayon::prelude::*;
use sha1::{Digest, Sha1};

const LEVEL: i32 = 19;
const MIB: usize = 1 << 20;

fn zstd(data: &[u8], window: usize) -> u64 {
    let mut encoder = zstd::bulk::Compressor::new(LEVEL).unwrap();
    encoder
        .set_parameter(zstd::zstd_safe::CParameter::WindowLog(window.ilog2()))
        .unwrap();
    encoder
        .set_parameter(zstd::zstd_safe::CParameter::ChecksumFlag(false))
        .unwrap();
    encoder.compress(data).unwrap().len() as u64
}

fn lzma2(data: &[u8], dict: usize) -> u64 {
    let mut options = Lzma2Options::with_preset(9);
    options.lzma_options.dict_size = dict as u32;
    let mut writer = Lzma2Writer::new(Vec::new(), options);
    std::io::Write::write_all(&mut writer, data).unwrap();
    writer.finish().unwrap().len() as u64
}

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
}

const CODECS: [Codec; 3] = [
    Codec::Zstd(MIB),
    Codec::Zstd(4 * MIB),
    Codec::Lzma2(4 * MIB),
];

/// `chunks` gathered in order into groups of `codec`'s size, each
/// compressed on its own: their total.
fn packed<'a>(chunks: impl Iterator<Item = &'a [u8]>, codec: Codec) -> u64 {
    let mut groups = vec![Vec::new()];
    for chunk in chunks {
        if groups.last().unwrap().len() + chunk.len() > codec.group() {
            groups.push(Vec::with_capacity(codec.group()));
        }
        groups.last_mut().unwrap().extend_from_slice(chunk);
    }
    groups
        .par_iter()
        .filter(|group| !group.is_empty())
        .map(|group| match codec {
            Codec::Zstd(n) => zstd(group, n),
            Codec::Lzma2(n) => lzma2(group, n),
        })
        .sum()
}

/// MinCDC as ore-rs cuts: 8 to 24 KiB.
fn cut(data: &[u8]) -> Vec<&[u8]> {
    SliceChunker::new(data, 8 << 10, 24 << 10, MinCdcHash4::new())
        .map(|chunk| &data[chunk.offset() as usize..][..chunk.len()])
        .collect()
}

fn digest(bytes: &[u8]) -> [u8; 20] {
    Sha1::digest(bytes).into()
}

/// Copy `chd` into `scratch` and extract it there, a BIN a track. A clone
/// takes its parent, found beside it by SHA-1, copied too. Returns the cue
/// sheet and whether it is a clone.
fn extract(chd: &Path, scratch: &Path) -> Result<(cue::Cue, bool)> {
    let _ = fs::remove_dir_all(scratch);
    fs::create_dir_all(scratch)?;
    let local = scratch.join("disc.chd");
    fs::copy(chd, &local).with_context(|| format!("copying {}", chd.display()))?;
    // A clone's header opens without its parent, and names it
    let parent = chd_rs::Chd::open(&local)?.info().parent_sha1;
    let mut opened = match parent {
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
    let sheet = scratch.join("disc.cue");
    chd_rs::extract_cd(&mut opened, &sheet, None, true, &mut |_| {})?;
    drop(opened);
    fs::remove_file(&local)?;
    let _ = fs::remove_file(scratch.join("parent.chd"));
    Ok((
        cue::parse(&fs::read_to_string(&sheet)?, scratch)?,
        parent.is_some(),
    ))
}

fn name(path: &Path) -> String {
    path.file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// What a disc's data tracks hold, split as an ORE stores them.
#[derive(Default)]
struct Split {
    sectors: u64,
    kinds: [u64; 6],
    subheader_copies_differ: u64,
    rebuild_failures: u64,
    /// The raw sectors, as they are.
    raw: Vec<u8>,
    /// What is left of each sector once generated parts are gone.
    data: Vec<u8>,
    /// Where each sector's data starts in `data`.
    starts: Vec<usize>,
    kinds_of: Vec<Kind>,
    /// Kinds, subheaders, differing second copies: what an index holds.
    meta: Vec<u8>,
}

fn split(cue: &cue::Cue) -> Result<Split> {
    let mut out = Split::default();
    let mut frame = LEAD_IN;
    for track in &cue.tracks {
        let bytes = fs::read(&track.file)?;
        frame += track.pregap;
        if track.mode == Mode::Audio {
            frame += (bytes.len() / SECTOR) as u32;
            continue;
        }
        let mut rebuilt = [0; SECTOR];
        for raw in bytes.as_chunks::<SECTOR>().0 {
            let kind = sector::classify(raw, frame);
            out.sectors += 1;
            out.kinds[kind as usize] += 1;
            out.meta.push(kind as u8);
            let mut subheader = [0; 8];
            if kind.has_subheader() {
                subheader.copy_from_slice(&raw[16..24]);
                out.meta.extend_from_slice(&subheader[..4]);
                if subheader[..4] != subheader[4..] {
                    out.subheader_copies_differ += 1;
                    out.meta.extend_from_slice(&subheader[4..]);
                }
            }
            let data = &raw[sector::data_range(kind)];
            sector::rebuild(kind, frame, &subheader, data, &mut rebuilt);
            if &rebuilt != raw {
                out.rebuild_failures += 1;
            }
            out.starts.push(out.data.len());
            out.kinds_of.push(kind);
            out.data.extend_from_slice(data);
            out.raw.extend_from_slice(raw);
            frame += 1;
        }
    }
    Ok(out)
}

/// What the ISO 9660 file system covers of the first data track.
#[derive(Default)]
struct FileSystem {
    entries: u64,
    files: u64,
    file_sectors: u64,
    directory_sectors: u64,
    form2_sectors: u64,
    interleaved_sectors: u64,
    /// Sectors no entry covers.
    outside: u64,
    /// Where files start, as offsets into the data plane.
    file_starts: Vec<usize>,
}

fn file_system(split: &Split) -> FileSystem {
    let mut read = |lba: u32, out: &mut [u8; 2048]| -> std::io::Result<()> {
        let at = lba as usize;
        match split.kinds_of.get(at) {
            Some(Kind::Mode1 | Kind::Form1) => {
                out.copy_from_slice(&split.data[split.starts[at]..][..FORM1_DATA]);
                Ok(())
            }
            _ => Err(std::io::Error::other("not a Form 1 sector")),
        }
    };
    let Ok(entries) = iso9660::entries(&mut read) else {
        return FileSystem::default();
    };
    let mut fs = FileSystem {
        entries: entries.len() as u64,
        ..FileSystem::default()
    };
    let mut covered = vec![false; split.sectors as usize];
    // The system area and the volume descriptors
    for c in covered.iter_mut().take(18) {
        *c = true;
    }
    for entry in &entries {
        let sectors = entry.sectors() as u64;
        if entry.directory {
            fs.directory_sectors += sectors;
        } else {
            fs.files += 1;
            fs.file_sectors += sectors;
            if entry.attributes & XA_FORM2 != 0 {
                fs.form2_sectors += sectors;
            }
            if entry.attributes & XA_INTERLEAVED != 0 {
                fs.interleaved_sectors += sectors;
            }
            if let Some(&start) = split.starts.get(entry.lba as usize) {
                fs.file_starts.push(start);
            }
        }
        for at in entry.lba as u64..entry.lba as u64 + sectors {
            if let Some(c) = covered.get_mut(at as usize) {
                *c = true;
            }
        }
    }
    fs.outside = covered.iter().filter(|&&c| !c).count() as u64;
    fs.file_starts.sort_unstable();
    fs.file_starts.dedup();
    fs
}

/// MinCDC chunks of the data plane, cut again at every file start.
fn cut_at_files<'a>(data: &'a [u8], starts: &[usize]) -> Vec<&'a [u8]> {
    let mut bounds: Vec<usize> = starts.iter().copied().filter(|&s| s < data.len()).collect();
    bounds.push(0);
    bounds.push(data.len());
    bounds.sort_unstable();
    bounds.dedup();
    bounds
        .windows(2)
        .flat_map(|w| cut(&data[w[0]..w[1]]))
        .collect()
}

/// Bytes and chunks of `chunks` not in `seen`, adding them.
fn fresh<'a>(chunks: &[&'a [u8]], seen: &mut HashSet<[u8; 20]>) -> Vec<&'a [u8]> {
    chunks
        .iter()
        .filter(|chunk| seen.insert(digest(chunk)))
        .copied()
        .collect()
}

fn data(scratch: &Path, chds: &[PathBuf]) -> Result<()> {
    print!(
        "disc\tchd\tclone\tsectors\traw\tzero\tmode1\tform1\tform2\tform2_no_edc\tsubheader_copies_differ\trebuild_failures"
    );
    print!(
        "\tentries\tfiles\tfile_sectors\tdirectory_sectors\tform2_file_sectors\tinterleaved_sectors\toutside"
    );
    print!(
        "\tdata_plane\tmeta_zstd\traw_z1\tunique_disc\tunique_disc_files\tunique_store\tunique_store_files"
    );
    for prefix in ["disc", "store"] {
        for codec in ["z1", "z4", "l4"] {
            print!("\t{prefix}_{codec}");
        }
    }
    println!();
    let (mut store, mut store_files) = (HashSet::new(), HashSet::new());
    for chd in chds {
        let disc = name(chd);
        eprintln!("{disc}");
        let chd_size = fs::metadata(chd)?.len();
        let (cue, clone) = extract(chd, scratch)?;
        let split = split(&cue)?;
        let fs = file_system(&split);
        let chunks = cut(&split.data);
        let file_chunks = cut_at_files(&split.data, &fs.file_starts);
        let own = fresh(&chunks, &mut HashSet::new());
        let own_files = fresh(&file_chunks, &mut HashSet::new());
        let new = fresh(&chunks, &mut store);
        let new_files = fresh(&file_chunks, &mut store_files);
        let bytes = |chunks: &[&[u8]]| chunks.iter().map(|c| c.len() as u64).sum::<u64>();
        let meta = zstd(&split.meta, 4 * MIB);
        let raw_z1 = packed(split.raw.chunks(SECTOR), Codec::Zstd(MIB));
        print!("{disc}\t{chd_size}\t{clone}\t{}", split.sectors);
        for count in split.kinds {
            print!("\t{count}");
        }
        print!(
            "\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            split.subheader_copies_differ,
            split.rebuild_failures,
            fs.entries,
            fs.files,
            fs.file_sectors,
            fs.directory_sectors,
            fs.form2_sectors,
            fs.interleaved_sectors,
            fs.outside
        );
        print!(
            "\t{}\t{meta}\t{raw_z1}\t{}\t{}\t{}\t{}",
            split.data.len(),
            bytes(&own),
            bytes(&own_files),
            bytes(&new),
            bytes(&new_files)
        );
        for chunks in [&own, &new] {
            for codec in CODECS {
                print!("\t{}", packed(chunks.iter().copied(), codec) + meta);
            }
        }
        println!();
        fs::remove_dir_all(scratch)?;
    }
    Ok(())
}

/// A WAV of raw CD audio: 44.1 kHz, 16 bits, stereo.
fn wav(pcm: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&44100u32.to_le_bytes());
    out.extend_from_slice(&(44100u32 * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(pcm);
    out
}

/// A codec as its tool: how to encode `in` to `out`, and decode `out` to
/// `back`.
struct Tool {
    name: &'static str,
    extension: &'static str,
    encode: fn(&Path, &Path) -> Command,
    decode: fn(&Path, &Path) -> Command,
}

fn command(program: &str, args: &[&std::ffi::OsStr]) -> Command {
    let mut command = Command::new(program);
    command.args(args);
    command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    command
}

macro_rules! os {
    ($($arg:expr),*) => { &[$(std::ffi::OsStr::new($arg)),*] };
}

const TOOLS: &[Tool] = &[
    Tool {
        name: "flac-8",
        extension: "flac",
        encode: |i, o| command("flac", os!("-8", "-s", "-f", "-o", o, i)),
        decode: |i, o| command("flac", os!("-d", "-s", "-f", "-o", o, i)),
    },
    Tool {
        name: "flac-8ep",
        extension: "flac",
        encode: |i, o| command("flac", os!("-8", "-e", "-p", "-s", "-f", "-o", o, i)),
        decode: |i, o| command("flac", os!("-d", "-s", "-f", "-o", o, i)),
    },
    Tool {
        name: "wavpack-hhx6",
        extension: "wv",
        encode: |i, o| command("wavpack", os!("-hh", "-x6", "-q", "-y", i, "-o", o)),
        decode: |i, o| command("wvunpack", os!("-q", "-y", i, "-o", o)),
    },
    Tool {
        name: "tta",
        extension: "tta",
        encode: |i, o| {
            command(
                "ffmpeg",
                os!("-y", "-loglevel", "quiet", "-i", i, "-c:a", "tta", o),
            )
        },
        decode: |i, o| command("ffmpeg", os!("-y", "-loglevel", "quiet", "-i", i, o)),
    },
    Tool {
        name: "mac-c4000",
        extension: "ape",
        encode: |i, o| command("mac", os!(i, o, "-c4000", "-threads=1")),
        decode: |i, o| command("mac", os!(i, o, "-d", "-threads=1")),
    },
    Tool {
        name: "mac-c5000",
        extension: "ape",
        encode: |i, o| command("mac", os!(i, o, "-c5000", "-threads=1")),
        decode: |i, o| command("mac", os!(i, o, "-d", "-threads=1")),
    },
    Tool {
        name: "ofr-max",
        extension: "ofr",
        encode: |i, o| {
            command(
                "ofr",
                os!("--encode", i, "--preset", "max", "--silent", "--output", o),
            )
        },
        decode: |i, o| command("ofr", os!("--decode", i, "--silent", "--output", o)),
    },
];

fn run(mut command: Command) -> Result<f64> {
    let start = Instant::now();
    let status = command.status()?;
    if !status.success() {
        bail!("{command:?} failed");
    }
    Ok(start.elapsed().as_secs_f64())
}

fn audio(scratch: &Path, chds: &[PathBuf]) -> Result<()> {
    println!("disc\ttrack\tpcm\tsha1\tcodec\tsize\tencode_seconds\tdecode_seconds\tlossless");
    for chd in chds {
        let disc = name(chd);
        eprintln!("{disc}");
        let (cue, _) = extract(chd, scratch)?;
        let tracks: Vec<_> = cue
            .tracks
            .iter()
            .filter(|t| t.mode == Mode::Audio)
            .collect();
        // A track a core, each tool alone on it, so the seconds are a core's
        let rows: Vec<String> = tracks
            .par_iter()
            .map(|track| -> Result<Vec<String>> {
                let pcm = fs::read(&track.file)?;
                let sha1 = hex(&digest(&pcm));
                let dir = scratch.join(format!("t{}", track.number));
                fs::create_dir_all(&dir)?;
                let input = dir.join("in.wav");
                fs::write(&input, wav(&pcm))?;
                let mut rows = Vec::new();
                for tool in TOOLS {
                    let encoded = dir.join(format!("{}.{}", tool.name, tool.extension));
                    let back = dir.join(format!("{}.wav", tool.name));
                    let encode = run((tool.encode)(&input, &encoded))?;
                    let decode = run((tool.decode)(&encoded, &back))?;
                    let decoded = fs::read(&back)?;
                    // The PCM the WAV holds, wherever its header ends
                    let lossless = decoded.len() >= pcm.len() && decoded.ends_with(&pcm);
                    rows.push(format!(
                        "{disc}\t{}\t{}\t{sha1}\t{}\t{}\t{encode:.2}\t{decode:.2}\t{lossless}",
                        track.number,
                        pcm.len(),
                        tool.name,
                        fs::metadata(&encoded)?.len()
                    ));
                    fs::remove_file(&encoded)?;
                    fs::remove_file(&back)?;
                }
                fs::remove_dir_all(&dir)?;
                Ok(rows)
            })
            .collect::<Result<Vec<_>>>()?
            .concat();
        for row in rows {
            println!("{row}");
        }
        fs::remove_dir_all(scratch)?;
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_bcd(byte: u8) -> u32 {
    (byte >> 4) as u32 * 10 + (byte & 15) as u32
}

fn sbi(paths: &[PathBuf]) -> Result<()> {
    println!("sbi\taddress\tstored\tgenerated\tdiffers");
    for path in paths {
        let bytes = fs::read(path)?;
        if bytes.len() < 4 || &bytes[..4] != b"SBI\0" {
            bail!("{}: not an SBI", path.display());
        }
        let mut at = 4;
        while at + 4 <= bytes.len() {
            let msf = &bytes[at..at + 3];
            let length = match bytes[at + 3] {
                1 => 10,
                2 => 3,
                3 => 11,
                other => bail!("{}: entry type {other}", path.display()),
            };
            let stored = &bytes[at + 4..at + 4 + length];
            at += 4 + length;
            let frame = (from_bcd(msf[0]) * 60 + from_bcd(msf[1])) * 75 + from_bcd(msf[2]);
            // Track 1, data, index 1, from 00:02:00
            let mut generated = vec![0x41, 0x01, 0x01];
            generated.extend_from_slice(&sector::address(frame - LEAD_IN));
            generated.push(0);
            generated.extend_from_slice(&sector::address(frame));
            let differs: Vec<String> = stored
                .iter()
                .zip(&generated)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .map(|(i, (a, b))| format!("{i}:{:02x}", a ^ b))
                .collect();
            println!(
                "{}\t{}\t{}\t{}\t{}",
                name(path),
                hex(msf),
                hex(stored),
                hex(&generated),
                differs.join(",")
            );
        }
    }
    Ok(())
}

/// Each disc's Form 2 sectors marked audio, as `OUT/<disc>.xa`: a sector's
/// subheader, 4 bytes, then its 2324 bytes of data. A row a disc: its Form 2
/// sectors by what their submode says they are.
fn xa(scratch: &Path, out: &Path, chds: &[PathBuf]) -> Result<()> {
    fs::create_dir_all(out)?;
    println!(
        "disc\tsectors\tform2\tform2_audio\tform2_video\tform2_data\tform2_other\taudio_4bit\taudio_stereo"
    );
    for chd in chds {
        let disc = name(chd);
        eprintln!("{disc}");
        let (cue, _) = extract(chd, scratch)?;
        let split = split(&cue)?;
        let mut counts = [0u64; 7];
        let mut dump = Vec::new();
        for (i, &kind) in split.kinds_of.iter().enumerate() {
            counts[0] += 1;
            if !matches!(kind, Kind::Form2 | Kind::Form2NoEdc) {
                continue;
            }
            counts[1] += 1;
            let raw = &split.raw[i * SECTOR..][..SECTOR];
            let (submode, coding) = (raw[18], raw[19]);
            let class = match () {
                _ if submode & 0x04 != 0 => 2,
                _ if submode & 0x02 != 0 => 3,
                _ if submode & 0x08 != 0 => 4,
                _ => 5,
            };
            counts[class] += 1;
            if class == 2 {
                counts[6] += u64::from(coding & 0x30 == 0);
                dump.extend_from_slice(&raw[16..20]);
                dump.extend_from_slice(&raw[24..24 + 2324]);
            }
        }
        let stereo = dump.chunks(2328).filter(|s| s[3] & 3 == 1).count();
        print!("{disc}");
        for count in counts {
            print!("\t{count}");
        }
        println!("\t{stereo}");
        fs::write(out.join(format!("{disc}.xa")), dump)?;
        fs::remove_dir_all(scratch)?;
    }
    Ok(())
}

/// A disc's video sectors of one header: how many, their data, and how
/// many of their frames' first chunks are each BS version.
type Found = (u64, Vec<u8>, [u64; 5]);

/// What a video sector's header says it is, as jpsxdec identifies them: a
/// StStatus of 0160h and a plausible chunk and frame, or Final Fantasy
/// VIII's `SMJ\1`. Its magic, and, for a frame's first chunk of the
/// standard header, its BS version.
fn video(data: &[u8]) -> Option<(u32, Option<u16>)> {
    let u16_at = |at: usize| u16::from_le_bytes([data[at], data[at + 1]]);
    let u32_at = |at: usize| u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
    if data.starts_with(b"SMJ\x01") {
        return Some((u32_at(0), None));
    }
    let (magic, chunk, chunks, frame) = (u32_at(0), u16_at(4), u16_at(6), u32_at(8));
    // 0160h, but not FF9's audio
    if magic & 0xFFFF != 0x0160 || magic == 0x0008_0160 || chunks == 0 || chunks > 64 {
        return None;
    }
    if chunk >= chunks || frame == 0 || frame > 1_000_000 {
        return None;
    }
    let version = (chunk == 0 && u16_at(0x16) == 0x3800).then(|| u16_at(0x1A));
    Some((magic, version))
}

/// Each disc's video sectors: how many, of which header and BS version,
/// what of the data plane, XA aside, they are, and what Zstandard makes of
/// them in 1 MiB groups, as ORE stores them now. A row a disc and header.
/// With `STR_DUMP` set, each disc's Form 1 video sectors' data also goes
/// to `STR_DUMP/<disc>.str`, for `str` to read.
fn str_video(scratch: &Path, chds: &[PathBuf]) -> Result<()> {
    let dump_dir = std::env::var_os("STR_DUMP").map(PathBuf::from);
    println!("disc\tplane\tmagic\tform2\tsectors\tbytes\tzstd\tframes\tv0\tv1\tv2\tv3\tother");
    for chd in chds {
        let disc = name(chd);
        eprintln!("{disc}");
        let (cue, _) = extract(chd, scratch)?;
        let split = split(&cue)?;
        let mut plane = 0u64;
        // By magic and form: sectors, data, versions of first chunks
        let mut found: std::collections::BTreeMap<(u32, bool), Found> = Default::default();
        let mut dump = Vec::new();
        for (i, &kind) in split.kinds_of.iter().enumerate() {
            let raw = &split.raw[i * SECTOR..][..SECTOR];
            let form2 = matches!(kind, Kind::Form2 | Kind::Form2NoEdc);
            if form2 && raw[18] & 0x04 != 0 && raw[19] & 0x30 == 0 {
                continue;
            }
            let data = &split.data[split.starts[i]..][..kind.data_len()];
            plane += data.len() as u64;
            if !matches!(
                kind,
                Kind::Form1 | Kind::Mode1 | Kind::Form2 | Kind::Form2NoEdc
            ) {
                continue;
            }
            if let Some((magic, version)) = video(data) {
                if !form2 && dump_dir.is_some() {
                    dump.extend_from_slice(data);
                }
                let entry = found.entry((magic, form2)).or_default();
                entry.0 += 1;
                entry.1.extend_from_slice(data);
                if let Some(version) = version {
                    entry.2[(version as usize).min(4)] += 1;
                }
            }
        }
        for ((magic, form2), (sectors, bytes, versions)) in &found {
            let zstd = packed(bytes.chunks(2048), Codec::Zstd(MIB));
            let frames: u64 = versions.iter().sum();
            print!(
                "{disc}\t{plane}\t{magic:08x}\t{form2}\t{sectors}\t{}\t{zstd}\t{frames}",
                bytes.len()
            );
            for count in versions {
                print!("\t{count}");
            }
            println!();
        }
        if found.is_empty() {
            println!("{disc}\t{plane}\t\t\t0\t0\t0\t0\t0\t0\t0\t0\t0");
        }
        if let Some(dir) = &dump_dir
            && !dump.is_empty()
        {
            fs::create_dir_all(dir)?;
            fs::write(dir.join(format!("{disc}.str")), &dump)?;
        }
        fs::remove_dir_all(scratch)?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let paths = |from: usize| args[from..].iter().map(PathBuf::from).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("data") if args.len() > 2 => data(Path::new(&args[1]), &paths(2)),
        Some("audio") if args.len() > 2 => audio(Path::new(&args[1]), &paths(2)),
        Some("xa") if args.len() > 3 => xa(Path::new(&args[1]), Path::new(&args[2]), &paths(3)),
        Some("str") if args.len() > 2 => str_video(Path::new(&args[1]), &paths(2)),
        Some("sbi") => sbi(&paths(1)),
        _ => bail!(
            "usage: cd data|audio SCRATCH <CHD>... | cd xa SCRATCH OUT <CHD>... | cd str SCRATCH <CHD>... | cd sbi <SBI>..."
        ),
    }
}
