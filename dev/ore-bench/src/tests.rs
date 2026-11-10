use super::*;

#[path = "../../../crates/gcwii/tests/common/mod.rs"]
mod common;

#[test]
fn levers_find_what_the_disc_holds() {
    // given a synthetic GameCube disc, padded everywhere nothing is
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("disc.iso");
    let mut disc = Disc(common::gamecube_disc(0x200000));
    common::write(&path, &disc.0);

    // when
    let layout = Layout::of(&mut disc).unwrap();
    let row = measure(
        &path,
        &directory.path().join("disc.rvz"),
        &disc,
        &layout,
        5,
        32 * 1024,
    )
    .unwrap();

    // then the padding and the boot DOL are found
    assert!(layout.filler_bytes() > disc.0.len() as u64 / 2);
    assert_eq!(layout.dol, Some((0x10000, 0x12100)));
    // and each lever weighs no more than the one before it, the padding
    // taking most of the disc away, as it does from the RVZ
    assert!(row.plain >= row.dedup && row.dedup >= row.filler);
    assert!(row.filler < row.dedup / 2);
    assert!(row.rvz < row.dedup / 2);
    assert!(!directory.path().join("disc.rvz").exists());
}
