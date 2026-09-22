//! Agreement with gdidrop itself: the same GDI descriptor, save for gdidrop's
//! ` [gdidrop]` file name suffix, and byte-identical tracks.
//!
//! Skipped unless dotnet and a gdidrop checkout are available (see
//! `common::reference`).

mod common;
use common::{CD, GD_ROM, NEAR_MISS, Spec, reference, run_reference, write_set};

fn same_as_gdidrop(specs: &[Spec]) {
    let Some(reference) = reference() else {
        eprintln!("skipping: no dotnet, or no gdidrop checkout (set $GDIDROP_SOURCE)");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in");
    let ours = dir.path().join("ours");
    std::fs::create_dir_all(&input).unwrap();
    std::fs::create_dir_all(&ours).unwrap();
    let cue = write_set(&input, "Game", specs);

    let gdi = gdi_rs::convert(&cue, &ours, &mut |_| {}).unwrap();
    // gdidrop writes next to the CUE, which is why it suffixes its tracks.
    run_reference(reference, &cue);

    let theirs = std::fs::read_to_string(input.join("Game.gdi")).unwrap();
    assert_eq!(
        std::fs::read_to_string(&gdi.gdi).unwrap(),
        theirs.replace(" [gdidrop]", "")
    );
    for track in &gdi.tracks {
        let stem = track.file_stem().unwrap().to_str().unwrap();
        let extension = track.extension().unwrap().to_str().unwrap();
        let theirs = input.join(format!("{stem} [gdidrop].{extension}"));
        assert!(
            std::fs::read(track).unwrap() == std::fs::read(&theirs).unwrap(),
            "{} differs from gdidrop's",
            track.display()
        );
    }
}

#[test]
fn gd_rom() {
    same_as_gdidrop(&GD_ROM);
}

#[test]
fn cd() {
    same_as_gdidrop(&CD);
}

#[test]
fn a_comment_that_only_looks_like_the_high_density_marker() {
    same_as_gdidrop(&NEAR_MISS);
}
