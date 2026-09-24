//! Conversion behaviour: gdidrop's layouts, pinned from what gdidrop itself
//! wrote for these sets when gdi-rs was ported, and what happens on bad input.

mod common;
use common::{CD, GD_ROM, NEAR_MISS, SECTOR, audio, byte, write_set, write_single};
use std::path::Path;

/// gdidrop's descriptor for `GD_ROM`: track 3 at 45000, after the jump the
/// high-density marker on track 2 causes, and every pregap skipped.
const GD_ROM_GDI: &str = "5
1 0 4 2352 \"Game (Track 1).bin\" 0
2 450 0 2352 \"Game (Track 2).raw\" 0
3 45000 4 2352 \"Game (Track 3).bin\" 0
4 45550 0 2352 \"Game (Track 4).raw\" 0
5 45975 4 2352 \"Game (Track 5).bin\" 0
";

#[test]
fn a_gd_rom_is_laid_out_as_gdidrop_does() {
    let dir = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let cue = write_set(dir.path(), "Game", &GD_ROM);

    let gdi = gdi_rs::convert(&cue, out.path(), &mut |_| {}).unwrap();

    assert_eq!(gdi.gdi, out.path().join("Game.gdi"));
    assert_eq!(std::fs::read_to_string(&gdi.gdi).unwrap(), GD_ROM_GDI);
    // Each track is its BIN from INDEX 01 on.
    for (i, (track, spec)) in gdi.tracks.iter().zip(GD_ROM).enumerate() {
        let data = std::fs::read(track).unwrap();
        assert_eq!(data.len() as u64, spec.sectors * SECTOR as u64);
        assert_eq!(data[0], byte(i, spec.pregap, 0), "{}", track.display());
    }
}

#[test]
fn other_layouts_are_laid_out_as_gdidrop_does() {
    for (specs, expected) in [
        // Track 1 past its pregap, track 2 whole.
        (
            &CD[..],
            "2\n1 231 4 2352 \"Game (Track 1).bin\" 0\n2 631 0 2352 \"Game (Track 2).raw\" 0\n",
        ),
        // gdidrop compares the high-density marker whole, so a comment only
        // containing it moves nothing: track 3 follows track 2.
        (
            &NEAR_MISS[..],
            "3\n1 0 4 2352 \"Game (Track 1).bin\" 0\n2 450 0 2352 \"Game (Track 2).raw\" 0\n3 750 4 2352 \"Game (Track 3).bin\" 0\n",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let cue = write_set(dir.path(), "Game", specs);
        let gdi = gdi_rs::convert(&cue, out.path(), &mut |_| {}).unwrap();
        assert_eq!(std::fs::read_to_string(&gdi.gdi).unwrap(), expected);
    }
}

#[test]
fn progress_adds_up_to_the_input_size() {
    let dir = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let cue = write_set(dir.path(), "Game", &GD_ROM);

    let mut read = 0;
    gdi_rs::convert(&cue, out.path(), &mut |n| read += n).unwrap();
    assert_eq!(read, gdi_rs::input_size(&cue).unwrap());
    let bins: u64 = GD_ROM
        .iter()
        .map(|spec| (spec.pregap + spec.sectors) * SECTOR as u64)
        .sum();
    assert_eq!(read, bins);
}

#[test]
fn a_data_track_is_never_written_over_its_own_bin() {
    let dir = tempfile::tempdir().unwrap();
    let cue = write_set(dir.path(), "Game", &CD);
    let bin = dir.path().join("Game (Track 1).bin");
    let before = std::fs::read(&bin).unwrap();

    let result = gdi_rs::convert(&cue, dir.path(), &mut |_| {});

    assert!(matches!(result, Err(gdi_rs::Error::InvalidOption(_))));
    assert!(std::fs::read(&bin).unwrap() == before);
    assert!(!dir.path().join("Game.gdi").exists());
}

#[test]
fn a_pregap_past_the_end_of_its_bin_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let cue = write_set(dir.path(), "Game", &CD);
    // The data track's BIN, cut short of its own pregap.
    std::fs::write(dir.path().join("Game (Track 1).bin"), vec![0; SECTOR * 100]).unwrap();

    let result = gdi_rs::convert(&cue, out.path(), &mut |_| {});

    assert!(matches!(result, Err(gdi_rs::Error::Corrupt(_))));
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
}

#[test]
fn a_failed_run_leaves_an_existing_set_alone() {
    let dir = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let cue = write_set(dir.path(), "Game", &[audio(0, 10), audio(0, 10)]);
    // A set already there, then track 2 cannot be written: a directory is in
    // the way of its `.part` file, after track 1's is written.
    std::fs::write(out.path().join("Game.gdi"), b"keep me").unwrap();
    std::fs::create_dir(out.path().join("Game (Track 2).raw.part")).unwrap();

    assert!(gdi_rs::convert(&cue, out.path(), &mut |_| {}).is_err());

    assert_eq!(
        std::fs::read(out.path().join("Game.gdi")).unwrap(),
        b"keep me"
    );
    assert!(!out.path().join("Game (Track 1).raw").exists());
    assert!(!out.path().join("Game (Track 1).raw.part").exists());
    // Not ours to remove.
    assert!(out.path().join("Game (Track 2).raw.part").is_dir());

    // Once nothing is in the way, a complete set replaces it.
    std::fs::remove_dir(out.path().join("Game (Track 2).raw.part")).unwrap();
    let gdi = gdi_rs::convert(&cue, out.path(), &mut |_| {}).unwrap();
    assert!(
        std::fs::read_to_string(&gdi.gdi)
            .unwrap()
            .starts_with("2\n")
    );
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 3);
}

#[test]
fn the_output_directory_must_exist() {
    let dir = tempfile::tempdir().unwrap();
    let cue = write_set(dir.path(), "Game", &CD);
    let result = gdi_rs::convert(&cue, &dir.path().join("missing"), &mut |_| {});
    assert!(matches!(result, Err(gdi_rs::Error::InvalidOption(_))));
}

/// A GDI descriptor with its file names left out.
fn layout(gdi: &Path) -> Vec<String> {
    std::fs::read_to_string(gdi)
        .unwrap()
        .lines()
        .map(|line| {
            let fields: Vec<&str> = line.split('"').collect();
            format!("{}{}", fields[0], fields.get(2).unwrap_or(&""))
        })
        .collect()
}

#[test]
fn a_single_bin_converts_as_if_redump_had_split_it() {
    for specs in [&GD_ROM[..], &CD[..]] {
        let split = tempfile::tempdir().unwrap();
        let single = tempfile::tempdir().unwrap();
        let (split_out, single_out) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let split_cue = write_set(split.path(), "Game", specs);
        let single_cue = write_single(single.path(), "Game", specs);

        let split_gdi = gdi_rs::convert(&split_cue, split_out.path(), &mut |_| {}).unwrap();
        let mut read = 0;
        let single_gdi =
            gdi_rs::convert(&single_cue, single_out.path(), &mut |n| read += n).unwrap();

        // The same tracks at the same places, which for the split set is
        // gdidrop's layout; only the names differ, the BIN naming them all.
        assert_eq!(layout(&single_gdi.gdi), layout(&split_gdi.gdi));
        for (single, split) in single_gdi.tracks.iter().zip(&split_gdi.tracks) {
            assert!(
                std::fs::read(single).unwrap() == std::fs::read(split).unwrap(),
                "{} differs from {}",
                single.display(),
                split.display()
            );
        }
        assert_eq!(
            single_gdi.tracks[1].file_name().unwrap(),
            if specs[1].audio {
                "Game (Track 02).raw"
            } else {
                "Game (Track 02).bin"
            }
        );
        // The BIN counted once, not once per track.
        let bin = std::fs::metadata(single.path().join("Game.bin"))
            .unwrap()
            .len();
        assert_eq!(read, bin);
        assert_eq!(gdi_rs::input_size(&single_cue).unwrap(), bin);
    }
}

#[test]
fn a_single_bin_can_be_split_next_to_itself() {
    // Its tracks are named after their number, so none lands on the BIN.
    let dir = tempfile::tempdir().unwrap();
    let cue = write_single(dir.path(), "Game", &CD);
    let before = std::fs::read(dir.path().join("Game.bin")).unwrap();

    gdi_rs::convert(&cue, dir.path(), &mut |_| {}).unwrap();

    assert!(std::fs::read(dir.path().join("Game.bin")).unwrap() == before);
    assert!(dir.path().join("Game (Track 01).bin").is_file());
}

#[test]
fn an_index_outside_its_share_of_the_bin_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Game.bin"), vec![0; SECTOR * 100]).unwrap();
    // Track 2 starts before track 1's INDEX 01.
    let cue = dir.path().join("Game.cue");
    std::fs::write(
        &cue,
        "FILE \"Game.bin\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 00 00:00:00\n    INDEX 01 00:00:50\n  TRACK 02 AUDIO\n    INDEX 01 00:00:20\n",
    )
    .unwrap();

    let result = gdi_rs::convert(&cue, out.path(), &mut |_| {});

    assert!(matches!(result, Err(gdi_rs::Error::Corrupt(_))));
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
}
