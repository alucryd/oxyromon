//! Rebuilding and extracting the synthetic disc, which test-support encrypts
//! as Redump's dumps are.

use std::path::{Path, PathBuf};

use md5::{Digest, Md5};
use ps3_rs::{Error, Ird, UPDATE_PATH};
use test_support::ps3;

fn setup() -> (tempfile::TempDir, ps3::Disc, Ird) {
    let disc = ps3::disc();
    let ird = Ird::parse(&disc.ird).unwrap();
    (tempfile::tempdir().unwrap(), disc, ird)
}

/// Where a rebuild finds the disc's files: the JB folder, and the update
/// beside it.
fn sources(jb: &Path, update: &Path) -> impl FnMut(&str) -> Option<PathBuf> {
    let (jb, update) = (jb.to_path_buf(), update.to_path_buf());
    move |path| {
        Some(if path == UPDATE_PATH {
            update.clone()
        } else {
            jb.join(path)
        })
    }
}

fn jb_and_update(dir: &Path, disc: &ps3::Disc) -> (PathBuf, PathBuf) {
    let jb = dir.join("Test Game");
    disc.write_jb(&jb);
    let update = dir.join("PS3UPDAT.PUP");
    std::fs::write(&update, disc.file(UPDATE_PATH)).unwrap();
    (jb, update)
}

#[test]
fn the_ird_reads_back_what_was_written() {
    let (_dir, disc, ird) = setup();
    assert_eq!(ird.game_id, ps3::GAME_ID);
    assert_eq!(ird.game_name, ps3::GAME_NAME);
    assert_eq!(ird.update_version, ps3::UPDATE_VERSION);
    assert_eq!(ird.disc_key(), ps3::DISC_KEY);
    assert_eq!(ird.size(), disc.iso.len() as u64);
    let files = ird.files().unwrap();
    let paths: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "PS3_DISC.SFB",
            "PS3_GAME/PARAM.SFO",
            "PS3_GAME/USRDIR/EBOOT.BIN",
            "PS3_GAME/USRDIR/Data/Big.dat",
            UPDATE_PATH,
        ]
    );
    // the file of two extents is one file of both
    assert_eq!(files[3].extents, [(60, 4096), (70, 3000)]);
    assert_eq!(files[3].size(), 7096);
    for file in &files {
        assert_eq!(
            ird.file_hash(file.lba()).unwrap(),
            <[u8; 16]>::from(Md5::digest(disc.file(&file.path)))
        );
    }
}

#[test]
fn a_jb_folder_and_its_ird_rebuild_the_iso() {
    let (dir, disc, ird) = setup();
    let (jb, update) = jb_and_update(dir.path(), &disc);
    let output = dir.path().join("Test Game.iso");
    let mut read = 0;
    let md5 = ps3_rs::rebuild(&ird, &mut sources(&jb, &update), &output, &mut |n| {
        read += n
    })
    .unwrap();
    let rebuilt = std::fs::read(&output).unwrap();
    assert!(rebuilt == disc.iso, "the rebuilt ISO differs");
    assert_eq!(md5, <[u8; 16]>::from(Md5::digest(&disc.iso)));
    let files: u64 = disc.files.iter().map(|(_, bytes)| bytes.len() as u64).sum();
    assert_eq!(read, files);
    assert!(!dir.path().join("Test Game.iso.part").exists());
}

#[test]
fn an_iso_extracts_to_its_jb_folder() {
    let (dir, disc, ird) = setup();
    let iso = dir.path().join("Test Game.iso");
    std::fs::write(&iso, &disc.iso).unwrap();
    let output = dir.path().join("Test Game");
    let mut read = 0;
    let files = ps3_rs::extract(&iso, &ird.disc_key(), &output, &mut |n| read += n).unwrap();
    assert_eq!(read, disc.iso.len() as u64);
    assert_eq!(files.len(), disc.files.len());
    for (file, md5) in &files {
        let expected = disc.file(&file.path);
        assert!(
            std::fs::read(output.join(&file.path)).unwrap() == expected,
            "{}",
            file.path
        );
        assert_eq!(*md5, <[u8; 16]>::from(Md5::digest(expected)));
    }
    assert!(!dir.path().join("Test Game.part").exists());
}

#[test]
fn a_wrong_file_fails_the_rebuild_and_leaves_nothing() {
    let (dir, disc, ird) = setup();
    let (jb, update) = jb_and_update(dir.path(), &disc);
    let eboot = jb.join("PS3_GAME/USRDIR/EBOOT.BIN");
    let mut bytes = std::fs::read(&eboot).unwrap();
    bytes[100] ^= 1;
    std::fs::write(&eboot, bytes).unwrap();
    let output = dir.path().join("Test Game.iso");
    let error =
        ps3_rs::rebuild(&ird, &mut sources(&jb, &update), &output, &mut |_| {}).unwrap_err();
    assert!(matches!(error, Error::WrongSource(_)), "{error}");
    assert!(!output.exists());
    assert!(!dir.path().join("Test Game.iso.part").exists());
}

#[test]
fn a_missing_file_fails_before_anything_is_written() {
    let (dir, disc, ird) = setup();
    let jb = dir.path().join("Test Game");
    disc.write_jb(&jb);
    let output = dir.path().join("Test Game.iso");
    // the JB folder has no update, and no other is given
    let error =
        ps3_rs::rebuild(&ird, &mut |path| Some(jb.join(path)), &output, &mut |_| {}).unwrap_err();
    assert!(matches!(error, Error::WrongSource(_)), "{error}");
    assert!(!output.exists());
    assert!(!dir.path().join("Test Game.iso.part").exists());
}

#[test]
fn a_wrong_key_fails_the_extraction_and_leaves_nothing() {
    let (dir, disc, _ird) = setup();
    let iso = dir.path().join("Test Game.iso");
    // the directories are plain, so the files are found; their contents are
    // garbage, which only their hashes tell, and the folder is still written
    std::fs::write(&iso, &disc.iso).unwrap();
    let output = dir.path().join("Test Game");
    let files = ps3_rs::extract(&iso, &[0; 16], &output, &mut |_| {}).unwrap();
    let eboot = files
        .iter()
        .find(|(file, _)| file.path == "PS3_GAME/USRDIR/EBOOT.BIN")
        .unwrap();
    assert_ne!(
        eboot.1,
        <[u8; 16]>::from(Md5::digest(disc.file("PS3_GAME/USRDIR/EBOOT.BIN")))
    );
    // a truncated disc fails, and leaves no folder behind
    std::fs::write(&iso, &disc.iso[..disc.iso.len() - 20 * ps3::SECTOR]).unwrap();
    std::fs::remove_dir_all(&output).unwrap();
    assert!(ps3_rs::extract(&iso, &[0; 16], &output, &mut |_| {}).is_err());
    assert!(!output.exists());
    assert!(!dir.path().join("Test Game.part").exists());
}

#[test]
fn a_pup_names_its_version() {
    let (dir, disc, _ird) = setup();
    let pup = dir.path().join("PS3UPDAT.PUP");
    std::fs::write(&pup, disc.file(UPDATE_PATH)).unwrap();
    assert_eq!(ps3_rs::pup_version(&pup).unwrap(), ps3::UPDATE_VERSION);
    let not = dir.path().join("not.pup");
    std::fs::write(&not, [0; 64]).unwrap();
    assert!(matches!(
        ps3_rs::pup_version(&not),
        Err(Error::BadMagic { .. })
    ));
}

#[test]
fn an_ird_damaged_fails_its_crc() {
    let (_dir, disc, _ird) = setup();
    let mut raw = Vec::new();
    std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(&disc.ird[..]), &mut raw).unwrap();
    raw[20] ^= 1;
    assert!(matches!(Ird::parse(&raw), Err(Error::Corrupt(_))));
}

#[test]
fn an_ird_inflating_past_its_bound_is_refused() {
    use std::io::Write;
    // a gzip bomb: little to read, far more than any IRD to inflate
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    let zeros = vec![0u8; 1 << 20];
    for _ in 0..=(ps3_rs::MAX_INFLATED >> 20) {
        encoder.write_all(&zeros).unwrap();
    }
    let bomb = encoder.finish().unwrap();
    assert!(bomb.len() < 1 << 20);
    assert!(matches!(Ird::parse(&bomb), Err(Error::Corrupt(_))));
}

#[test]
fn a_disc_key_is_told_right_by_what_eboot_decrypts_to() {
    let (_dir, disc, ird) = setup();
    let mut read = |lba: u32, sector: &mut [u8; ps3::SECTOR]| {
        let at = lba as usize * ps3::SECTOR;
        sector.copy_from_slice(&disc.iso[at..at + ps3::SECTOR]);
        Ok(())
    };
    let probe = ps3_rs::disc::Probe::find(&mut read).unwrap().unwrap();
    assert!(probe.opens(&ird.disc_key()));
    assert!(!probe.opens(&[0x55; 16]));
}
