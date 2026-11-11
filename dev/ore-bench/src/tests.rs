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

#[test]
fn inventory_lists_the_system_area_and_every_file() {
    // given a synthetic GameCube disc: a DOL, a file system and three files
    let mut disc = Disc(common::gamecube_disc(0x200000));

    // when
    let entries = inventory::inventory(&mut disc).unwrap();

    // then each is listed in disc order, the file of one repeated byte
    // compressing to almost nothing
    let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names, ["<boot>", "<dol>", "<fst>", "a", "b", "c"]);
    let b = &entries[4];
    assert_eq!((b.offset, b.size), (0x50000, 0x8000));
    assert!(b.compressed < 100);
}

#[test]
fn kinds_come_from_magic_bytes() {
    assert_eq!(inventory::kind(b"Yaz0\0\0\0\x10"), "yaz0");
    assert_eq!(inventory::kind(b"THP\0\0\x01"), "thp");
    let mut adx = vec![0x80, 0x00, 0x00, 0x1C];
    adx.resize(0x20, 0);
    adx[0x1A..0x20].copy_from_slice(b"(c)CRI");
    assert_eq!(inventory::kind(&adx), "adx");
    assert_eq!(inventory::kind(&[0; 16]), "zero");
    assert_eq!(inventory::kind(b"other"), "?");
}
