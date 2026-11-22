//! A Wii U eShop title through the C ABI: its files listed and read from the
//! byte view, and its ticket, come back as its NUS folder had them, so an
//! emulator mounts the ORE as the folder.

#[path = "../../wiiu/tests/common/mod.rs"]
mod wiiu_common;

use std::ffi::{CStr, CString};
use std::path::Path;
use std::ptr;

use ore_rs::Options;
use smelt::*;
use tempfile::TempDir;
use wiiu_common::COMMON_KEY;

fn open(path: &Path) -> *mut Ingot {
    let path = CString::new(path.to_str().unwrap()).unwrap();
    let mut ingot = ptr::null_mut();
    assert_eq!(
        unsafe { smelt_open(path.as_ptr(), ptr::null(), &mut ingot) },
        SmeltErr::SmeltOk
    );
    ingot
}

/// Every file in `dir`, with its contents, sorted by name.
fn folder(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read(&path).unwrap())
        })
        .collect();
    files.sort();
    files
}

/// The title's files and its ticket, read through `ingot` as C reads them.
fn through_smelt(ingot: *mut Ingot) -> Vec<(String, Vec<u8>)> {
    let mut count = 0;
    assert_eq!(
        unsafe { smelt_file_count(ingot, &mut count) },
        SmeltErr::SmeltOk
    );
    let mut files = Vec::new();
    for i in 0..count {
        // The name's length asked for first, its place given with it
        let (mut len, mut offset, mut size) = (0, 0, 0);
        assert_eq!(
            unsafe { smelt_file_at(ingot, i, ptr::null_mut(), &mut len, &mut offset, &mut size) },
            SmeltErr::SmeltErrBufferTooSmall
        );
        let mut name = vec![0u8; len];
        assert_eq!(
            unsafe {
                smelt_file_at(
                    ingot,
                    i,
                    name.as_mut_ptr().cast(),
                    &mut len,
                    &mut offset,
                    &mut size,
                )
            },
            SmeltErr::SmeltOk
        );
        let name = CStr::from_bytes_with_nul(&name).unwrap();
        let mut bytes = vec![0u8; size as usize];
        assert_eq!(
            unsafe { smelt_read(ingot, offset, bytes.as_mut_ptr(), bytes.len()) },
            SmeltErr::SmeltOk
        );
        files.push((name.to_string_lossy().into_owned(), bytes));
    }
    let (mut name_len, mut offset, mut size) = (0, 0, 0);
    assert_eq!(
        unsafe {
            smelt_file_at(
                ingot,
                count,
                ptr::null_mut(),
                &mut name_len,
                &mut offset,
                &mut size,
            )
        },
        SmeltErr::SmeltErrRange
    );
    let mut len = 0;
    assert_eq!(
        unsafe { smelt_ticket(ingot, ptr::null_mut(), &mut len) },
        SmeltErr::SmeltErrBufferTooSmall
    );
    let mut ticket = vec![0u8; len];
    assert_eq!(
        unsafe { smelt_ticket(ingot, ticket.as_mut_ptr(), &mut len) },
        SmeltErr::SmeltOk
    );
    files.push(("title.tik".into(), ticket));
    files.sort();
    files
}

#[test]
fn a_title_reads_back_file_by_file_with_its_ticket() {
    let dir = TempDir::new().unwrap();
    let nus = dir.path().join("Update");
    std::fs::create_dir(&nus).unwrap();
    wiiu_common::nus(&nus);
    let ore = dir.path().join("Update.ore");
    ore_rs::compress(&nus, &ore, &Options::default(), &[COMMON_KEY], &mut |_| {}).unwrap();

    let ingot = open(&ore);
    assert!(through_smelt(ingot) == folder(&nus));
    // A clone lists the same files
    let mut clone = ptr::null_mut();
    assert_eq!(unsafe { smelt_clone(ingot, &mut clone) }, SmeltErr::SmeltOk);
    assert!(through_smelt(clone) == folder(&nus));
    unsafe {
        smelt_close(clone);
        smelt_close(ingot);
    }
}

#[test]
fn a_title_without_its_ticket_lists_its_files_and_no_ticket() {
    let dir = TempDir::new().unwrap();
    let nus = dir.path().join("DLC");
    std::fs::create_dir(&nus).unwrap();
    wiiu_common::nus(&nus);
    std::fs::remove_file(nus.join("title.tik")).unwrap();
    let ore = dir.path().join("DLC.ore");
    ore_rs::compress(&nus, &ore, &Options::default(), &[], &mut |_| {}).unwrap();

    let ingot = open(&ore);
    let mut count = 0;
    assert_eq!(
        unsafe { smelt_file_count(ingot, &mut count) },
        SmeltErr::SmeltOk
    );
    assert_eq!(count, folder(&nus).len());
    let mut len = 0;
    assert_eq!(
        unsafe { smelt_ticket(ingot, ptr::null_mut(), &mut len) },
        SmeltErr::SmeltErrUnsupported
    );
    unsafe { smelt_close(ingot) };
}
