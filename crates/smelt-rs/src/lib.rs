// SPDX-License-Identifier: MIT OR Apache-2.0
//! smelt, the ORE reader library: ore-rs's reader behind a C ABI, so an
//! emulator reads an ORE as the disc or ROM it holds, at any offset, with no
//! temporary file. ore is unrefined iron; smelting refines it.
//!
//! Two views of an ORE. The byte view, for everything: open, clone, close,
//! the size, the platform and the hashes of what it holds, and reads. The
//! drive view, for a CD: its tracks, whole 2,352-byte sectors by LBA as a
//! drive reads them, and the Q subchannel its SBI replaces, libcrypt's.
//!
//! An ingot belongs to one thread at a time: [`Reader::read_at`] caches what
//! it decodes, so every read changes it. [`smelt_clone`] gives another
//! thread an ingot of its own over the same file and store, rather than a
//! lock inside serialising an emulator's drive thread against the rest.
//!
//! No ABI is stable before ORE's specification is; [`smelt_version`] says
//! which build a host has.

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use ore_rs::{Error, Reader, Track, TrackMode};

/// What a call came to.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmeltErr {
    SmeltOk = 0,
    /// A pointer NULL, a path not UTF-8, or options not understood.
    SmeltErrArgument = 1,
    /// The file could not be read.
    SmeltErrIo = 2,
    /// The file is no ORE.
    SmeltErrNotOre = 3,
    /// An ORE this build cannot read, or a hash it does not record.
    SmeltErrUnsupported = 4,
    /// The ORE, or its store, is damaged.
    SmeltErrCorrupt = 5,
    /// A thin ORE whose store is not found above it, or is another.
    SmeltErrMissingStore = 6,
    /// A read past the end of what the ORE holds.
    SmeltErrRange = 7,
    /// The buffer is smaller than the length now in `*len`.
    SmeltErrBufferTooSmall = 8,
    /// smelt itself failed; the ingot should be closed.
    SmeltErrPanic = 9,
}

/// What a CD track's sectors hold.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmeltTrackMode {
    SmeltTrackAudio = 0,
    SmeltTrackMode1 = 1,
    SmeltTrackMode2 = 2,
}

/// A CD track, as its cue sheet and the ORE give it. LBAs count from
/// 00:02:00, and a GD-ROM's high-density area starts at LBA 45,000.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SmeltTrack {
    pub number: u8,
    /// In a GD-ROM's high-density area: 1, or 0.
    pub high_density: u8,
    /// Its Q subchannel's control bits, the high nibble of Q's first
    /// byte: its cue sheet's FLAGS, 4CH 0x8, DCP 0x2, PRE 0x1, and 0x4
    /// for a data track.
    pub control: u8,
    pub mode: SmeltTrackMode,
    /// Where its index 01 lies.
    pub lba: u32,
    /// The sectors of pregap held before index 01, from `lba - pregap`.
    pub pregap: u32,
    /// Its sectors from index 01 to its end.
    pub sectors: u32,
    /// Where its first sector, its pregap's if held, starts in the byte
    /// view, in bytes.
    pub offset: u64,
}

/// The bytes of a raw CD sector, which smelt_read_sectors gives a sector.
pub const SMELT_SECTOR: u32 = 2352;

use SmeltErr::*;

/// Options to [`smelt_open`]; pass NULL to take every default.
#[repr(C)]
pub struct OpenOptions {
    /// Bytes of decoded groups the ingot keeps in memory; 0 is the
    /// default, 32 MiB. Android frontends over a slow share want less.
    pub cache_bytes: u32,
    /// Reserved: 0.
    pub reserved: u32,
}

/// A sector's frame from 00:00:00, and the 10 bytes of Q its SBI gives it.
type Replacement = (u64, [u8; 10]);

/// An open ORE.
pub struct Ingot {
    reader: Reader,
    /// A CD's tracks, none for what is not one.
    tracks: Vec<Track>,
    /// Its SBI's replacements by frame, in order, or why it has none.
    subq: std::result::Result<Vec<Replacement>, (SmeltErr, String)>,
}

impl Ingot {
    fn new(reader: Reader) -> std::result::Result<Self, Error> {
        let tracks = reader.tracks()?;
        let subq = reader
            .subchannel_q()
            .map(|mut entries| {
                entries.sort_unstable_by_key(|&(frame, _)| frame);
                entries
            })
            .map_err(|error| (code(&error), error.to_string()));
        Ok(Self {
            reader,
            tracks,
            subq,
        })
    }

    /// The track whose file holds the sector at `lba`, and that file's
    /// first LBA.
    fn track_at(&self, lba: u64) -> Option<(&Track, u64)> {
        self.tracks.iter().find_map(|track| {
            let first = track.frame.checked_sub(LEAD_IN)?;
            (first <= lba && lba < first + track.sectors).then_some((track, first))
        })
    }
}

/// LBA 0's frame from 00:00:00.
const LEAD_IN: u64 = 150;

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

fn fail(code: SmeltErr, message: impl Into<String>) -> SmeltErr {
    let message = CString::new(message.into().replace('\0', " ")).unwrap_or_default();
    LAST_ERROR.with(|last| *last.borrow_mut() = message);
    code
}

fn code(error: &Error) -> SmeltErr {
    match error {
        Error::Io(_) => SmeltErrIo,
        Error::BadMagic { .. } => SmeltErrNotOre,
        Error::Corrupt(_) => SmeltErrCorrupt,
        _ => SmeltErrUnsupported,
    }
}

fn fail_with(error: Error) -> SmeltErr {
    fail(code(&error), error.to_string())
}

/// Run `f`, a panic in it an error rather than unwinding into C.
fn guard(f: impl FnOnce() -> SmeltErr) -> SmeltErr {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| fail(SmeltErrPanic, "smelt panicked"))
}

/// Copy `bytes` to `buf`, of the capacity in `*len`, `*len` then the length
/// needed.
///
/// # Safety
///
/// `len` is valid, and `buf`, when not NULL, holds `*len` bytes.
unsafe fn give(bytes: &[u8], buf: *mut u8, len: *mut usize) -> SmeltErr {
    let capacity = unsafe { *len };
    unsafe { *len = bytes.len() };
    if buf.is_null() || capacity < bytes.len() {
        return fail(
            SmeltErrBufferTooSmall,
            format!("{} bytes needed, {capacity} given", bytes.len()),
        );
    }
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len()) };
    SmeltOk
}

/// This build's version, `major << 16 | minor << 8 | patch`.
#[unsafe(no_mangle)]
pub extern "C" fn smelt_version() -> u32 {
    let part = |s: &str| s.parse::<u32>().unwrap_or(0);
    part(env!("CARGO_PKG_VERSION_MAJOR")) << 16
        | part(env!("CARGO_PKG_VERSION_MINOR")) << 8
        | part(env!("CARGO_PKG_VERSION_PATCH"))
}

/// The calling thread's last error, as text, "" if none: valid until its
/// next call into smelt.
#[unsafe(no_mangle)]
pub extern "C" fn smelt_last_error() -> *const c_char {
    LAST_ERROR.with(|last| last.borrow().as_ptr())
}

/// Open the ORE at `path`, UTF-8, and if it is thin, the store its chunks
/// lie in, the nearest `.ore-store` up from its folder. `opts` is NULL
/// for the defaults or options this build knows.
///
/// # Safety
///
/// `path` is a NUL-terminated string, `opts` NULL or valid, `out` valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn smelt_open(
    path: *const c_char,
    opts: *const OpenOptions,
    out: *mut *mut Ingot,
) -> SmeltErr {
    guard(|| {
        if path.is_null() || out.is_null() {
            return fail(SmeltErrArgument, "a NULL argument");
        }
        let cache_bytes = if opts.is_null() {
            0
        } else {
            if unsafe { (*opts).reserved } != 0 {
                return fail(SmeltErrArgument, "options this build does not know");
            }
            (unsafe { (*opts).cache_bytes }) as usize
        };
        let Ok(path) = unsafe { CStr::from_ptr(path) }.to_str() else {
            return fail(SmeltErrArgument, "a path not UTF-8");
        };
        let path = Path::new(path);
        let reader = match Reader::open_with(path, cache_bytes) {
            Ok(reader) => reader,
            // The ORE reads, but not the store it needs
            Err(Error::Io(error))
                if error.kind() == std::io::ErrorKind::NotFound
                    && Reader::inspect(path).is_ok_and(|ore| ore.store_uuid().is_some()) =>
            {
                return fail(SmeltErrMissingStore, error.to_string());
            }
            Err(error) => return fail_with(error),
        };
        match Ingot::new(reader) {
            Ok(ingot) => {
                unsafe { *out = Box::into_raw(Box::new(ingot)) };
                SmeltOk
            }
            Err(error) => fail_with(error),
        }
    })
}

/// Another ingot over the same ORE, for another thread, sharing a thin
/// ORE's store index.
///
/// # Safety
///
/// `ingot` is an open ingot no other thread is using, `out` valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn smelt_clone(ingot: *const Ingot, out: *mut *mut Ingot) -> SmeltErr {
    guard(|| {
        let (Some(ingot), false) = (unsafe { ingot.as_ref() }, out.is_null()) else {
            return fail(SmeltErrArgument, "a NULL argument");
        };
        match ingot.reader.try_clone() {
            Ok(reader) => {
                let clone = Ingot {
                    reader,
                    tracks: ingot.tracks.clone(),
                    subq: ingot.subq.clone(),
                };
                unsafe { *out = Box::into_raw(Box::new(clone)) };
                SmeltOk
            }
            Err(error) => fail_with(error),
        }
    })
}

/// Close an ingot; NULL is ignored.
///
/// # Safety
///
/// `ingot` is NULL or an open ingot, not used after.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn smelt_close(ingot: *mut Ingot) {
    if !ingot.is_null() {
        drop(unsafe { Box::from_raw(ingot) });
    }
}

/// The size in bytes of what the ORE holds: a disc image's, a ROM's, or a
/// CD's tracks end to end.
///
/// # Safety
///
/// `ingot` is an open ingot, `out` valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn smelt_logical_size(ingot: *const Ingot, out: *mut u64) -> SmeltErr {
    guard(|| {
        let (Some(ingot), false) = (unsafe { ingot.as_ref() }, out.is_null()) else {
            return fail(SmeltErrArgument, "a NULL argument");
        };
        unsafe { *out = ingot.reader.size() };
        SmeltOk
    })
}

/// The platform the ORE was written as, NUL-terminated: `gamecube`, `ps2`,
/// `snes` and so on, as ore-rs names them.
///
/// # Safety
///
/// `ingot` is an open ingot, `len` valid, `buf` NULL or of `*len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn smelt_platform(
    ingot: *const Ingot,
    buf: *mut c_char,
    len: *mut usize,
) -> SmeltErr {
    guard(|| {
        let (Some(ingot), false) = (unsafe { ingot.as_ref() }, len.is_null()) else {
            return fail(SmeltErrArgument, "a NULL argument");
        };
        let Ok(platform) = CString::new(ingot.reader.platform()) else {
            return fail(SmeltErrCorrupt, "a platform holding a NUL");
        };
        unsafe { give(platform.as_bytes_with_nul(), buf.cast(), len) }
    })
}

/// The hash `scheme` of what the ORE holds, as recorded when it was
/// written: `blake3-256`, `sha-1`, `md5` or `crc-32` of a disc image or a
/// ROM, `blake3-256` of a CD's tracks end to end. A claim, not checked here.
///
/// # Safety
///
/// `ingot` is an open ingot, `scheme` a NUL-terminated string, `len` valid,
/// `buf` NULL or of `*len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn smelt_identity(
    ingot: *const Ingot,
    scheme: *const c_char,
    buf: *mut u8,
    len: *mut usize,
) -> SmeltErr {
    guard(|| {
        let (Some(ingot), false, false) =
            (unsafe { ingot.as_ref() }, scheme.is_null(), len.is_null())
        else {
            return fail(SmeltErrArgument, "a NULL argument");
        };
        let scheme = unsafe { CStr::from_ptr(scheme) }.to_bytes();
        let found = ingot
            .reader
            .identity()
            .find(|&(view, algo, _)| view == "logical" && algo.as_bytes() == scheme);
        match found {
            Some((_, _, value)) => unsafe { give(value, buf, len) },
            None => fail(
                SmeltErrUnsupported,
                format!(
                    "no {} recorded",
                    String::from_utf8_lossy(scheme).into_owned()
                ),
            ),
        }
    })
}

/// Read `len` bytes at `offset` of what the ORE holds into `dst`:
/// SMELT_ERR_RANGE past its end, before anything is written. On any other
/// error, `dst` may hold some of the bytes: discard it.
///
/// # Safety
///
/// `ingot` is an open ingot no other thread is using, `dst` of `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn smelt_read(
    ingot: *mut Ingot,
    offset: u64,
    dst: *mut u8,
    len: usize,
) -> SmeltErr {
    guard(|| {
        let Some(ingot) = (unsafe { ingot.as_mut() }) else {
            return fail(SmeltErrArgument, "a NULL argument");
        };
        if len == 0 {
            return SmeltOk;
        }
        if dst.is_null() {
            return fail(SmeltErrArgument, "a NULL argument");
        }
        let size = ingot.reader.size();
        if offset.checked_add(len as u64).is_none_or(|end| end > size) {
            return fail(
                SmeltErrRange,
                format!("{len} bytes at {offset}, past {size}"),
            );
        }
        let dst = unsafe { std::slice::from_raw_parts_mut(dst, len) };
        match ingot.reader.read_at(offset, dst) {
            Ok(()) => SmeltOk,
            Err(error) => fail_with(error),
        }
    })
}

/// A CD's tracks: 0 for what is not a CD.
///
/// # Safety
///
/// `ingot` is an open ingot, `out` valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn smelt_track_count(ingot: *const Ingot, out: *mut usize) -> SmeltErr {
    guard(|| {
        let (Some(ingot), false) = (unsafe { ingot.as_ref() }, out.is_null()) else {
            return fail(SmeltErrArgument, "a NULL argument");
        };
        unsafe { *out = ingot.tracks.len() };
        SmeltOk
    })
}

/// The `i`th track, from 0, into `out`: SMELT_ERR_RANGE past the last.
///
/// # Safety
///
/// `ingot` is an open ingot, `out` valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn smelt_track_at(
    ingot: *const Ingot,
    i: usize,
    out: *mut SmeltTrack,
) -> SmeltErr {
    guard(|| {
        let (Some(ingot), false) = (unsafe { ingot.as_ref() }, out.is_null()) else {
            return fail(SmeltErrArgument, "a NULL argument");
        };
        let Some(track) = ingot.tracks.get(i) else {
            return fail(
                SmeltErrRange,
                format!("track {i} of {}", ingot.tracks.len()),
            );
        };
        let Some(first) = track.frame.checked_sub(LEAD_IN) else {
            return fail(
                SmeltErrCorrupt,
                format!("track {} before LBA 0", track.number),
            );
        };
        unsafe {
            *out = SmeltTrack {
                number: track.number,
                high_density: track.high_density.into(),
                control: track.flags
                    | if track.mode == TrackMode::Audio {
                        0
                    } else {
                        0x4
                    },
                mode: match track.mode {
                    TrackMode::Audio => SmeltTrackMode::SmeltTrackAudio,
                    TrackMode::Mode1 => SmeltTrackMode::SmeltTrackMode1,
                    TrackMode::Mode2 => SmeltTrackMode::SmeltTrackMode2,
                },
                lba: (first + track.index1) as u32,
                pregap: track.index1 as u32,
                sectors: track.sectors.saturating_sub(track.index1) as u32,
                offset: track.offset,
            }
        };
        SmeltOk
    })
}

/// Read `n` whole sectors of a CD from `lba` into `dst`, 2,352 bytes each,
/// across tracks: SMELT_ERR_RANGE for a sector no track's file holds, as a
/// pregap the dump leaves out, before anything is written;
/// SMELT_ERR_UNSUPPORTED for what is not a CD. On any other error, `dst`
/// may hold some of the sectors: discard it.
///
/// # Safety
///
/// `ingot` is an open ingot no other thread is using, `dst` of
/// `n * SMELT_SECTOR` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn smelt_read_sectors(
    ingot: *mut Ingot,
    lba: u32,
    n: u32,
    dst: *mut u8,
) -> SmeltErr {
    guard(|| {
        let Some(ingot) = (unsafe { ingot.as_mut() }) else {
            return fail(SmeltErrArgument, "a NULL argument");
        };
        if ingot.tracks.is_empty() {
            return fail(SmeltErrUnsupported, "not a CD");
        }
        if n == 0 {
            return SmeltOk;
        }
        if dst.is_null() {
            return fail(SmeltErrArgument, "a NULL argument");
        }
        let sector = SMELT_SECTOR as usize;
        let dst = unsafe { std::slice::from_raw_parts_mut(dst, n as usize * sector) };
        // Checked whole first, so a read past a track's end writes nothing
        let (mut at, end) = (u64::from(lba), u64::from(lba) + u64::from(n));
        let mut spans = Vec::new();
        while at < end {
            let Some((track, first)) = ingot.track_at(at) else {
                return fail(SmeltErrRange, format!("sector {at} is in no track's file"));
            };
            let count = end.min(first + track.sectors) - at;
            spans.push((track.offset + (at - first) * sector as u64, count));
            at += count;
        }
        let mut written = 0;
        for (offset, count) in spans {
            let len = count as usize * sector;
            if let Err(error) = ingot
                .reader
                .read_at(offset, &mut dst[written..written + len])
            {
                return fail_with(error);
            }
            written += len;
        }
        SmeltOk
    })
}

/// The Q subchannel of the sector at `lba`, its 10 bytes without their CRC,
/// into `q` with `*replaced` 1 when the CD's SBI replaces it, as libcrypt
/// protected discs have it; `*replaced` 0 and `q` untouched when the drive
/// should generate it from the sector's position, as for every other.
///
/// # Safety
///
/// `ingot` is an open ingot, `q` of 10 bytes, `replaced` valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn smelt_subq_replacement(
    ingot: *const Ingot,
    lba: u32,
    q: *mut u8,
    replaced: *mut u8,
) -> SmeltErr {
    guard(|| {
        let (Some(ingot), false, false) =
            (unsafe { ingot.as_ref() }, q.is_null(), replaced.is_null())
        else {
            return fail(SmeltErrArgument, "a NULL argument");
        };
        let entries = match &ingot.subq {
            Ok(entries) => entries,
            Err((code, error)) => return fail(*code, error.clone()),
        };
        let frame = u64::from(lba) + LEAD_IN;
        match entries.binary_search_by_key(&frame, |&(frame, _)| frame) {
            Ok(i) => unsafe {
                std::ptr::copy_nonoverlapping(entries[i].1.as_ptr(), q, 10);
                *replaced = 1;
            },
            Err(_) => unsafe { *replaced = 0 },
        }
        SmeltOk
    })
}
