//! The drive view of a CD ORE: its tracks as its cue sheet gives them,
//! whole sectors by LBA across them, and the Q subchannel its SBI replaces.

use std::ffi::CString;
use std::path::Path;
use std::ptr;

use ore_rs::Options;
use smelt::*;
use tempfile::TempDir;

const SECTOR: usize = SMELT_SECTOR as usize;
const DATA: usize = 40;
const AUDIO: usize = 200;

fn noise(len: usize, mut state: u64) -> Vec<u8> {
    (0..len)
        .map(|_| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (state >> 56) as u8
        })
        .collect()
}

/// A data track of noise and an audio track whose file holds its two
/// seconds of pregap, copying permitted, with an SBI replacing LBA 10's Q: the disc's bytes,
/// its tracks end to end, and its ORE.
fn cd(dir: &Path) -> (Vec<u8>, std::path::PathBuf) {
    let mut sbi = b"SBI\0".to_vec();
    // 00:02:10, LBA 10, type 1, then its Q
    sbi.extend_from_slice(&[0x00, 0x02, 0x10, 0x01]);
    sbi.extend_from_slice(&[0x41, 0x01, 0x01, 0x00, 0x00, 0x10, 0x00, 0x00, 0x02, 0x10]);
    cd_with(dir, &sbi)
}

/// [`cd`] with the SBI `sbi`.
fn cd_with(dir: &Path, sbi: &[u8]) -> (Vec<u8>, std::path::PathBuf) {
    let data = noise(DATA * SECTOR, 3);
    let audio = noise(AUDIO * SECTOR, 4);
    std::fs::write(dir.join("Game (Track 1).bin"), &data).unwrap();
    std::fs::write(dir.join("Game (Track 2).bin"), &audio).unwrap();
    let cue = dir.join("Game.cue");
    std::fs::write(
        &cue,
        "FILE \"Game (Track 1).bin\" BINARY\r\n  TRACK 01 MODE1/2352\r\n    INDEX 01 00:00:00\r\n\
         FILE \"Game (Track 2).bin\" BINARY\r\n  TRACK 02 AUDIO\r\n    FLAGS DCP\r\n    INDEX 00 00:00:00\r\n    INDEX 01 00:02:00\r\n",
    )
    .unwrap();
    std::fs::write(dir.join("Game.sbi"), sbi).unwrap();
    let ore = dir.join("Game.ore");
    ore_rs::compress(&cue, &ore, &Options::default(), &[], &mut |_| {}).unwrap();
    ([data, audio].concat(), ore)
}

fn open(path: &Path) -> *mut Ingot {
    let mut ingot = ptr::null_mut();
    let path = CString::new(path.to_str().unwrap()).unwrap();
    assert_eq!(
        unsafe { smelt_open(path.as_ptr(), ptr::null(), &mut ingot) },
        SmeltErr::SmeltOk
    );
    ingot
}

#[test]
fn a_cd_reads_as_a_drive_reads_it() {
    let dir = TempDir::new().unwrap();
    let (bytes, ore) = cd(dir.path());
    let ingot = open(&ore);

    let mut count = 0;
    assert_eq!(
        unsafe { smelt_track_count(ingot, &mut count) },
        SmeltErr::SmeltOk
    );
    assert_eq!(count, 2);
    let mut tracks = [SmeltTrack {
        number: 0,
        high_density: 0,
        control: 0,
        mode: SmeltTrackMode::SmeltTrackAudio,
        lba: 0,
        pregap: 0,
        sectors: 0,
        offset: 0,
    }; 2];
    for (i, track) in tracks.iter_mut().enumerate() {
        assert_eq!(
            unsafe { smelt_track_at(ingot, i, track) },
            SmeltErr::SmeltOk
        );
    }
    let [data, audio] = tracks;
    assert_eq!(
        (
            data.number,
            data.mode,
            data.lba,
            data.pregap,
            data.sectors,
            data.offset
        ),
        (1, SmeltTrackMode::SmeltTrackMode1, 0, 0, DATA as u32, 0)
    );
    // Its index 01 two seconds into its file, the file right after track 1's
    assert_eq!(
        (
            audio.number,
            audio.mode,
            audio.lba,
            audio.pregap,
            audio.sectors
        ),
        (
            2,
            SmeltTrackMode::SmeltTrackAudio,
            DATA as u32 + 150,
            150,
            AUDIO as u32 - 150
        )
    );
    assert_eq!(audio.offset, (DATA * SECTOR) as u64);
    assert_eq!(audio.high_density, 0);
    // Q's control bits: data, then its FLAGS DCP
    assert_eq!((data.control, audio.control), (0x4, 0x2));
    assert_eq!(
        unsafe { smelt_track_at(ingot, 2, &mut tracks[0]) },
        SmeltErr::SmeltErrRange
    );

    // Two sectors across the tracks, then the whole disc, then one past it
    let mut two = vec![0u8; 2 * SECTOR];
    let lba = DATA as u32 - 1;
    assert_eq!(
        unsafe { smelt_read_sectors(ingot, lba, 2, two.as_mut_ptr()) },
        SmeltErr::SmeltOk
    );
    assert!(two[..] == bytes[(DATA - 1) * SECTOR..(DATA + 1) * SECTOR]);
    let mut all = vec![0u8; bytes.len()];
    let n = (DATA + AUDIO) as u32;
    assert_eq!(
        unsafe { smelt_read_sectors(ingot, 0, n, all.as_mut_ptr()) },
        SmeltErr::SmeltOk
    );
    assert!(all == bytes);
    assert_eq!(
        unsafe { smelt_read_sectors(ingot, n - 1, 2, two.as_mut_ptr()) },
        SmeltErr::SmeltErrRange
    );

    // LBA 10's Q is the SBI's; LBA 11's is the drive's to generate
    let mut q = [0u8; 10];
    let mut replaced = 9;
    assert_eq!(
        unsafe { smelt_subq_replacement(ingot, 10, q.as_mut_ptr(), &mut replaced) },
        SmeltErr::SmeltOk
    );
    assert_eq!(replaced, 1);
    assert_eq!(
        q,
        [0x41, 0x01, 0x01, 0x00, 0x00, 0x10, 0x00, 0x00, 0x02, 0x10]
    );
    assert_eq!(
        unsafe { smelt_subq_replacement(ingot, 11, q.as_mut_ptr(), &mut replaced) },
        SmeltErr::SmeltOk
    );
    assert_eq!(replaced, 0);
    unsafe { smelt_close(ingot) };
}

#[test]
fn what_is_no_cd_has_no_drive_view() {
    let dir = TempDir::new().unwrap();
    let rom = dir.path().join("Game.nes");
    std::fs::write(&rom, noise(32 << 10, 5)).unwrap();
    let ore = dir.path().join("Game.ore");
    let options = Options {
        platform: Some("nes"),
        ..Options::default()
    };
    ore_rs::compress(&rom, &ore, &options, &[], &mut |_| {}).unwrap();
    let ingot = open(&ore);
    let mut count = 9;
    assert_eq!(
        unsafe { smelt_track_count(ingot, &mut count) },
        SmeltErr::SmeltOk
    );
    assert_eq!(count, 0);
    let mut sector = vec![0u8; SECTOR];
    assert_eq!(
        unsafe { smelt_read_sectors(ingot, 0, 1, sector.as_mut_ptr()) },
        SmeltErr::SmeltErrUnsupported
    );
    let (mut q, mut replaced) = ([0u8; 10], 9);
    assert_eq!(
        unsafe { smelt_subq_replacement(ingot, 0, q.as_mut_ptr(), &mut replaced) },
        SmeltErr::SmeltOk
    );
    assert_eq!(replaced, 0);
    unsafe { smelt_close(ingot) };
}

#[test]
fn an_sbi_smelt_cannot_take_is_refused_where_it_is_asked_for() {
    let header = |kind: u8, frame: u8| [0x00, 0x02, frame, kind];
    let q = [0x41, 0x01, 0x01, 0x00, 0x00, 0x10, 0x00, 0x00, 0x02, 0x10];
    for (name, sbi, code) in [
        // A partial replacement, its relative address alone
        (
            "type 2",
            [&b"SBI\0"[..], &header(2, 0x10), &q[..3]].concat(),
            SmeltErr::SmeltErrUnsupported,
        ),
        // An entry cut short
        (
            "short",
            [&b"SBI\0"[..], &header(1, 0x10), &q[..6]].concat(),
            SmeltErr::SmeltErrCorrupt,
        ),
        // Frame 75 of a second, and a nibble past 9
        (
            "frame",
            [&b"SBI\0"[..], &header(1, 0x75), &q[..]].concat(),
            SmeltErr::SmeltErrCorrupt,
        ),
        (
            "nibble",
            [&b"SBI\0"[..], &header(1, 0x1A), &q[..]].concat(),
            SmeltErr::SmeltErrCorrupt,
        ),
    ] {
        let dir = TempDir::new().unwrap();
        let (bytes, ore) = cd_with(dir.path(), &sbi);
        // The disc reads all the same
        let ingot = open(&ore);
        let mut sector = vec![0u8; SECTOR];
        assert_eq!(
            unsafe { smelt_read_sectors(ingot, 0, 1, sector.as_mut_ptr()) },
            SmeltErr::SmeltOk
        );
        assert!(sector[..] == bytes[..SECTOR]);
        let (mut q, mut replaced) = ([0u8; 10], 9);
        assert_eq!(
            unsafe { smelt_subq_replacement(ingot, 10, q.as_mut_ptr(), &mut replaced) },
            code,
            "{name}"
        );
        unsafe { smelt_close(ingot) };
    }
}
