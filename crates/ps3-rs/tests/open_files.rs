//! A rebuild holds one of a JB folder's files open at a time, so a disc of
//! thousands, LittleBigPlanet 2's 3,897, stays under the limit of open
//! files. Its own test binary, a process of its own, as it lowers that
//! limit for the whole process.

#![cfg(target_os = "linux")]

use ps3_rs::{Ird, UPDATE_PATH};
use test_support::ps3;

/// What this process holds open.
fn open_files() -> u64 {
    // counting its own handle on the folder too
    std::fs::read_dir("/proc/self/fd").unwrap().count() as u64
}

#[test]
fn a_rebuild_holds_one_file_open_at_a_time() {
    let dir = tempfile::tempdir().unwrap();
    let disc = ps3::disc();
    let ird = Ird::parse(&disc.ird).unwrap();
    let jb = dir.path().join("Test Game");
    disc.write_jb(&jb);
    let update = dir.path().join("PS3UPDAT.PUP");
    std::fs::write(&update, disc.file(UPDATE_PATH)).unwrap();
    let output = dir.path().join("Test Game.iso");

    // Room for the output and one file more than is open now: the disc's
    // five files held together would need four more
    let mut old = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut old) }, 0);
    let low = libc::rlimit {
        rlim_cur: open_files() + 2,
        rlim_max: old.rlim_max,
    };
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &low) }, 0);

    let result = ps3_rs::rebuild(
        &ird,
        &mut |path| {
            Some(if path == UPDATE_PATH {
                update.clone()
            } else {
                jb.join(path)
            })
        },
        &output,
        &mut |_| {},
    );
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &old) }, 0);

    result.unwrap();
    assert!(
        std::fs::read(&output).unwrap() == disc.iso,
        "the ISO differs"
    );
}
