//! smelt, the ORE reader library: ore-rs's reader behind a C ABI, so an
//! emulator reads an ORE as the disc or ROM it holds, at any offset, with no
//! temporary file. ore is unrefined iron; smelting refines it.
//!
//! This is the byte view, the first of design 04's API: open, clone, close,
//! the size, the platform and the hashes of what the ORE holds, and reads.
//! The drive view, a CD's geometry, whole sectors and channels, comes next.
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

use ore_rs::{Error, Reader};

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

use SmeltErr::*;

/// Options to [`smelt_open`]; none yet, so pass NULL.
#[repr(C)]
pub struct OpenOptions {
    /// Reserved: 0.
    pub reserved: u32,
}

/// An open ORE.
pub struct Ingot {
    reader: Reader,
}

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

fn fail(code: SmeltErr, message: impl Into<String>) -> SmeltErr {
    let message = CString::new(message.into().replace('\0', " ")).unwrap_or_default();
    LAST_ERROR.with(|last| *last.borrow_mut() = message);
    code
}

fn fail_with(error: Error) -> SmeltErr {
    let code = match &error {
        Error::Io(_) => SmeltErrIo,
        Error::BadMagic { .. } => SmeltErrNotOre,
        Error::Corrupt(_) => SmeltErrCorrupt,
        _ => SmeltErrUnsupported,
    };
    fail(code, error.to_string())
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
/// lie in, the nearest `.ore-store` up from its folder. `opts` is NULL.
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
        if !opts.is_null() && unsafe { (*opts).reserved } != 0 {
            return fail(SmeltErrArgument, "options this build does not know");
        }
        let Ok(path) = unsafe { CStr::from_ptr(path) }.to_str() else {
            return fail(SmeltErrArgument, "a path not UTF-8");
        };
        let path = Path::new(path);
        let reader = match Reader::open(path) {
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
        unsafe { *out = Box::into_raw(Box::new(Ingot { reader })) };
        SmeltOk
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
                unsafe { *out = Box::into_raw(Box::new(Ingot { reader })) };
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

/// Read `len` bytes at `offset` of what the ORE holds into `dst`, all or
/// nothing: SMELT_ERR_RANGE past its end.
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
