//! The C ABI, called as C calls it: an ORE read through an ingot comes back
//! as ore-rs gives it, and every failure is its error code.

use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr;

use ore_rs::Options;
use smelt::*;
use tempfile::TempDir;

/// 96 KiB of a headerless ROM, named a NES's so ore-rs takes it whole.
fn rom(dir: &Path) -> (PathBuf, Vec<u8>) {
    let mut state = 11u64;
    let mut bytes: Vec<u8> = (0..64 << 10)
        .map(|_| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (state >> 56) as u8
        })
        .collect();
    bytes.resize(96 << 10, 0xFF);
    let path = dir.join("Game (USA).nes");
    std::fs::write(&path, &bytes).unwrap();
    (path, bytes)
}

fn options() -> Options {
    Options {
        platform: Some("nes"),
        ..Options::default()
    }
}

fn c(path: &Path) -> CString {
    CString::new(path.to_str().unwrap()).unwrap()
}

fn open(path: &Path) -> Result<*mut Ingot, SmeltErr> {
    let mut ingot = ptr::null_mut();
    match unsafe { smelt_open(c(path).as_ptr(), ptr::null(), &mut ingot) } {
        SmeltErr::SmeltOk => Ok(ingot),
        error => Err(error),
    }
}

fn last_error() -> String {
    unsafe { CStr::from_ptr(smelt_last_error()) }
        .to_string_lossy()
        .into_owned()
}

#[test]
fn an_ore_reads_through_an_ingot_as_ore_rs_reads_it() {
    let dir = TempDir::new().unwrap();
    let (input, bytes) = rom(dir.path());
    let ore = dir.path().join("game.ore");
    ore_rs::compress(&input, &ore, &options(), &[], &mut |_| {}).unwrap();
    let ingot = open(&ore).unwrap();
    assert_eq!(smelt_version(), 1 << 8);

    let mut size = 0;
    assert_eq!(
        unsafe { smelt_logical_size(ingot, &mut size) },
        SmeltErr::SmeltOk
    );
    assert_eq!(size, bytes.len() as u64);

    // The platform, asked for its length first
    let mut len = 0;
    assert_eq!(
        unsafe { smelt_platform(ingot, ptr::null_mut(), &mut len) },
        SmeltErr::SmeltErrBufferTooSmall
    );
    assert_eq!(len, 4);
    let mut platform = vec![0u8; len];
    assert_eq!(
        unsafe { smelt_platform(ingot, platform.as_mut_ptr().cast(), &mut len) },
        SmeltErr::SmeltOk
    );
    assert_eq!(platform, b"nes\0");

    // Its SHA-1, as ore-rs recorded it, and a hash it did not
    let expected = ore_rs::Reader::open(&ore)
        .unwrap()
        .identity()
        .find(|&(view, algo, _)| view == "logical" && algo == "sha-1")
        .unwrap()
        .2
        .to_vec();
    let mut sha1 = [0u8; 20];
    let mut len = 19;
    let scheme = CString::new("sha-1").unwrap();
    assert_eq!(
        unsafe { smelt_identity(ingot, scheme.as_ptr(), sha1.as_mut_ptr(), &mut len) },
        SmeltErr::SmeltErrBufferTooSmall
    );
    assert_eq!(len, 20);
    assert_eq!(
        unsafe { smelt_identity(ingot, scheme.as_ptr(), sha1.as_mut_ptr(), &mut len) },
        SmeltErr::SmeltOk
    );
    assert_eq!(sha1.to_vec(), expected);
    let scheme = CString::new("sha-256").unwrap();
    assert_eq!(
        unsafe { smelt_identity(ingot, scheme.as_ptr(), sha1.as_mut_ptr(), &mut len) },
        SmeltErr::SmeltErrUnsupported
    );
    assert!(last_error().contains("sha-256"), "{}", last_error());

    // The whole ROM, a read across the end of its noise, and one past it
    let mut back = vec![0u8; bytes.len()];
    assert_eq!(
        unsafe { smelt_read(ingot, 0, back.as_mut_ptr(), back.len()) },
        SmeltErr::SmeltOk
    );
    assert!(back == bytes);
    let mut window = [0u8; 4096];
    let at = (64 << 10) - 100;
    assert_eq!(
        unsafe { smelt_read(ingot, at, window.as_mut_ptr(), window.len()) },
        SmeltErr::SmeltOk
    );
    assert!(window[..] == bytes[at as usize..at as usize + 4096]);
    assert_eq!(
        unsafe { smelt_read(ingot, size - 10, window.as_mut_ptr(), 11) },
        SmeltErr::SmeltErrRange
    );
    assert_eq!(
        unsafe { smelt_read(ingot, u64::MAX, window.as_mut_ptr(), 1) },
        SmeltErr::SmeltErrRange
    );

    // A clone reads on another thread while the first is closed
    let mut clone = ptr::null_mut();
    assert_eq!(unsafe { smelt_clone(ingot, &mut clone) }, SmeltErr::SmeltOk);
    unsafe { smelt_close(ingot) };
    let clone = clone as usize;
    let read = std::thread::spawn(move || {
        let clone = clone as *mut Ingot;
        let mut back = vec![0u8; 96 << 10];
        let code = unsafe { smelt_read(clone, 0, back.as_mut_ptr(), back.len()) };
        unsafe { smelt_close(clone) };
        (code, back)
    })
    .join()
    .unwrap();
    assert_eq!(read.0, SmeltErr::SmeltOk);
    assert!(read.1 == bytes);
}

#[test]
fn a_thin_ore_away_from_its_store_is_a_missing_store() {
    let dir = TempDir::new().unwrap();
    let (input, bytes) = rom(dir.path());
    let games = dir.path().join("games");
    let store = games.join(".ore-store");
    ore_rs::create_store(&store, &Default::default()).unwrap();
    let ore = games.join("game.ore");
    ore_rs::compress_to_store(&input, &ore, &store, &options(), &[], &mut |_| {}).unwrap();

    // Beneath its store, it reads
    let ingot = open(&ore).unwrap();
    let mut back = vec![0u8; bytes.len()];
    assert_eq!(
        unsafe { smelt_read(ingot, 0, back.as_mut_ptr(), back.len()) },
        SmeltErr::SmeltOk
    );
    assert!(back == bytes);
    unsafe { smelt_close(ingot) };

    // Away from it, it does not, naming the store it needs
    let away = dir.path().join("game.ore");
    std::fs::copy(&ore, &away).unwrap();
    assert_eq!(open(&away), Err(SmeltErr::SmeltErrMissingStore));
    assert!(last_error().contains("ORE store"), "{}", last_error());
}

#[test]
fn what_is_no_ore_or_no_argument_is_refused() {
    let dir = TempDir::new().unwrap();
    let (input, _) = rom(dir.path());
    assert_eq!(open(&input), Err(SmeltErr::SmeltErrNotOre));
    assert_eq!(
        open(&dir.path().join("missing.ore")),
        Err(SmeltErr::SmeltErrIo)
    );
    let mut ingot = ptr::null_mut();
    assert_eq!(
        unsafe { smelt_open(ptr::null(), ptr::null(), &mut ingot) },
        SmeltErr::SmeltErrArgument
    );
    // Options this build does not know
    let ore = dir.path().join("game.ore");
    ore_rs::compress(&input, &ore, &options(), &[], &mut |_| {}).unwrap();
    let opts = OpenOptions {
        cache_bytes: 0,
        reserved: 1,
    };
    assert_eq!(
        unsafe { smelt_open(c(&ore).as_ptr(), &opts, &mut ingot) },
        SmeltErr::SmeltErrArgument
    );
    let mut size = 0;
    assert_eq!(
        unsafe { smelt_logical_size(ptr::null(), &mut size) },
        SmeltErr::SmeltErrArgument
    );
    unsafe { smelt_close(ptr::null_mut()) };
}

#[test]
fn a_cache_budget_smaller_than_a_group_still_reads() {
    let dir = TempDir::new().unwrap();
    let (input, bytes) = rom(dir.path());
    let ore = dir.path().join("game.ore");
    ore_rs::compress(&input, &ore, &options(), &[], &mut |_| {}).unwrap();
    // 4 KiB caches nothing this ORE can keep: every group is re-read,
    // and the disc still comes back byte for byte
    let opts = OpenOptions {
        cache_bytes: 4 << 10,
        reserved: 0,
    };
    let mut ingot = ptr::null_mut();
    assert_eq!(
        unsafe { smelt_open(c(&ore).as_ptr(), &opts, &mut ingot) },
        SmeltErr::SmeltOk
    );
    let mut back = vec![0u8; bytes.len()];
    assert_eq!(
        unsafe { smelt_read(ingot, 0, back.as_mut_ptr(), back.len()) },
        SmeltErr::SmeltOk
    );
    assert!(back == bytes);
    unsafe { smelt_close(ingot) };
}
