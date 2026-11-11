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

#[test]
fn dedup_finds_a_disc_again_whatever_the_scheme() {
    // given the rows of one synthetic disc, listed twice as two discs
    let disc = Disc(common::gamecube_disc(0x200000));
    let mut rows = String::from("disc\tscheme\tdigest\tlength\tcompressed\n");
    for name in ["one", "two"] {
        let mut copy = Disc(disc.0.clone());
        let codecs = ["zstd19", "lzma2"];
        for (scheme, units) in dedup::units(&mut copy, dedup::SCHEMES, &codecs).unwrap() {
            for unit in units {
                for (codec, compressed) in codecs.iter().zip(&unit.compressed) {
                    rows += &format!(
                        "{name}\t{scheme}/{codec}\t{:016x}\t{}\t{compressed}\n",
                        unit.digest, unit.length
                    );
                }
            }
        }
    }

    // when
    let totals = dedup::sum(rows.as_bytes()).unwrap();

    // then every scheme stores the second disc as references only: deduped,
    // the pair weighs one disc's units and both discs' references
    assert_eq!(totals.len(), dedup::SCHEMES.len() * 2);
    for (scheme, total) in totals {
        let references = total.references * dedup::REFERENCE;
        assert_eq!(2 * total.library - total.per_disc, references, "{scheme}");
        // and the padding, most of the disc, is cut out of what is counted
        assert!(total.length < 0x200000, "{scheme}");
    }
}

#[test]
fn a_second_copy_costs_little_in_one_solid_stream() {
    // given the same synthetic disc twice
    let directory = tempfile::TempDir::new().unwrap();
    let data = common::gamecube_disc(0x200000);
    let (one, two) = (
        directory.path().join("one.iso"),
        directory.path().join("two.iso"),
    );
    common::write(&one, &data);
    common::write(&two, &data);
    let discs = [one.as_path(), two.as_path()];

    // when
    let alone = solid::alone(&discs).unwrap();
    let together = solid::together(&discs).unwrap();
    let (deduped, references, stored) = solid::deduped(&discs, 16 << 10).unwrap();

    // then one stream holding both weighs about one disc, deduped or not
    assert_eq!(alone[0], alone[1]);
    assert!(
        together < alone[0] + alone[0] / 10,
        "{together} against {}",
        alone[0]
    );
    assert_eq!(references, 2 * stored);
    assert!(deduped < together + references * dedup::REFERENCE + stored * dedup::STORED + 64);
}
