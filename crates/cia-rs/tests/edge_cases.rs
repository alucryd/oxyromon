//! The paths a real dump rarely takes and the reference tool met only by
//! crashing or reading out of bounds: malformed headers, malformed TMDs,
//! contents that do not fit. Each must come back as a clean error naming
//! what is wrong, never a panic.

use cia_rs::{Cia, CiaFormat, Error, Part, Section, UnpackOptions, unpack};

mod common;
use common::{Sections, cia, put16, put32, rec, tmd, write_cia};

fn parse(data: &[u8]) -> Result<Cia, Error> {
    let dir = tempfile::tempdir().unwrap();
    let path = write_cia(dir.path(), data);
    Cia::parse(&path, &mut |_| {})
}

fn corrupt(error: &Error, message: &str) -> bool {
    matches!(error, Error::Corrupt(text) if text == message)
}

/// A complete container's parts, so a test can break exactly one thing.
fn full() -> Sections {
    let payload: Vec<u8> = (0..64u32).map(|i| i as u8).collect();
    let meta = tmd(0x0004_0000_0F80_0100, 3, &[rec(0x1234, 0, &payload)]);
    Sections {
        certs: vec![0u8; 128],
        tik: vec![9u8; 256],
        tmd: meta,
        contents: vec![payload],
        footer: vec![8u8; 64],
    }
}

#[test]
fn a_file_too_small_for_a_header() {
    let error = parse(&[0u8; 0x1000]).unwrap_err();
    assert!(
        matches!(&error, Error::Corrupt(t) if t.contains("too small")),
        "{error}"
    );
}

#[test]
fn a_header_size_that_is_not_the_ctr_one() {
    let mut data = cia(0, &[0], &full());
    put32(&mut data, 0x00, 0x100);
    let error = parse(&data).unwrap_err();
    assert!(
        matches!(&error, Error::Corrupt(t) if t.contains("header declares a size of 256")),
        "{error}"
    );
}

#[test]
fn a_container_type_that_is_not_the_ctr_one() {
    let mut data = cia(0, &[0], &full());
    put16(&mut data, 0x04, 1);
    match parse(&data).unwrap_err() {
        Error::Unsupported(text) => assert!(text.contains("type 0x0001"), "{text}"),
        error => panic!("expected unsupported, got {error}"),
    }
}

#[test]
fn a_format_version_nobody_shipped() {
    let data = cia(1, &[0], &full());
    match parse(&data).unwrap_err() {
        Error::Unsupported(text) => assert!(text.contains("format version 0x0001"), "{text}"),
        error => panic!("expected unsupported, got {error}"),
    }
}

#[test]
fn a_file_shorter_than_its_sizes_announce() {
    let mut data = cia(0, &[0], &full());
    put32(&mut data, 0x08, 0x10000); // certificates that are not there
    let error = parse(&data).unwrap_err();
    assert!(
        matches!(&error, Error::Corrupt(t) if t.contains("too small for the size")),
        "{error}"
    );
}

#[test]
fn a_normal_cia_missing_a_part_it_cannot_do_without() {
    for (name, missing) in [("ticket", 1usize), ("title metadata", 2), ("contents", 3)] {
        let sections = Sections {
            certs: if missing == 0 { vec![] } else { full().certs },
            tik: if missing == 1 { vec![] } else { full().tik },
            tmd: if missing == 2 { vec![] } else { full().tmd },
            contents: if missing == 3 {
                vec![]
            } else {
                full().contents
            },
            footer: vec![],
        };
        let data = cia(0, &[0], &sections);
        let error = parse(&data).unwrap_err();
        assert!(corrupt(&error, &format!("CIA has no {name}")), "{error}");
    }
}

#[test]
fn a_simple_cia_is_one_content_with_nothing_describing_it() {
    let payload: Vec<u8> = (0..0x1000u32).map(|i| i as u8).collect();
    let data = cia(
        0xFF,
        &[],
        &Sections {
            contents: vec![payload.clone()],
            ..Sections::default()
        },
    );
    let cia = parse(&data).unwrap();
    assert_eq!(cia.format(), CiaFormat::Simple);
    assert_eq!(cia.title_id(), 0);
    assert_eq!(cia.title_version(), 0);
    assert_eq!(cia.contents().len(), 1);
    assert_eq!(cia.contents()[0].size, 0x1000);
    // The first section is aligned up like every other: 0x2020 is not a
    // multiple of 64.
    assert_eq!(cia.contents()[0].offset, 0x2040);
    assert_eq!(cia.section(Section::Footer), None);

    let dir = tempfile::tempdir().unwrap();
    let path = write_cia(dir.path(), &data);
    let out = dir.path().join("out");
    let written = unpack(&path, &out, &UnpackOptions::default(), &mut |_| {}).unwrap();
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].path, out.join("content.0000.00000000"));
    assert_eq!(std::fs::read(&written[0].path).unwrap(), payload);
    assert!(!out.join("tmd").exists());
}

#[test]
fn a_simple_cia_with_no_content() {
    let data = cia(
        0xFF,
        &[],
        &Sections {
            certs: vec![1u8; 64],
            ..Sections::default()
        },
    );
    let error = parse(&data).unwrap_err();
    assert!(corrupt(&error, "CIA has no contents"), "{error}");
}

/// Where the parser will find this file's TMD section.
fn tmd_at(data: &[u8]) -> usize {
    let mut pos = 0x2020usize;
    for size in [0x08, 0x0C].map(|at| u32::from_le_bytes(data[at..at + 4].try_into().unwrap())) {
        if size == 0 {
            continue;
        }
        pos = pos.div_ceil(64) * 64 + size as usize;
    }
    pos.div_ceil(64) * 64
}

#[test]
fn a_tmd_signed_with_a_type_that_is_not_a_signature() {
    let mut data = cia(0, &[0], &full());
    let at = tmd_at(&data);
    data[at..at + 4].copy_from_slice(&0x0001_0006u32.to_be_bytes()); // HMAC-SHA1
    match parse(&data).unwrap_err() {
        Error::Unsupported(text) => assert!(text.contains("TMD signature type"), "{text}"),
        error => panic!("expected unsupported, got {error}"),
    }
}

#[test]
fn a_tmd_of_an_unknown_format_version() {
    let mut data = cia(0, &[0], &full());
    let at = tmd_at(&data);
    data[at + 384] = 2;
    match parse(&data).unwrap_err() {
        Error::Unsupported(text) => assert!(text.contains("TMD format version 2"), "{text}"),
        error => panic!("expected unsupported, got {error}"),
    }
}

#[test]
fn a_tmd_too_small_for_even_one_record() {
    let mut sections = full();
    // The group array is whole, but the last record is missing — the size
    // the reference first refused a TMD at.
    sections.tmd.truncate(sections.tmd.len() - 48);
    let error = parse(&cia(0, &[0], &sections)).unwrap_err();
    assert!(corrupt(&error, "TMD is too small"), "{error}");
}

#[test]
fn a_tmd_whose_group_array_hash_lies() {
    let mut data = cia(0, &[0], &full());
    let at = tmd_at(&data);
    data[at + 384 + 100 + 0x20] ^= 0xFF; // first byte of the group array
    let error = parse(&data).unwrap_err();
    assert!(
        corrupt(&error, "TMD had an invalid CMD group hash"),
        "{error}"
    );
}

#[test]
fn a_tmd_whose_record_hash_lies() {
    let mut data = cia(0, &[0], &full());
    let at = tmd_at(&data);
    data[at + 384 + 2436 + 20] ^= 0xFF; // inside the first record
    let error = parse(&data).unwrap_err();
    assert!(
        corrupt(&error, "TMD had an invalid CMD group hash"),
        "{error}"
    );
}

#[test]
fn a_content_that_extends_past_the_container() {
    let payload = vec![7u8; 64];
    let mut record = rec(1, 0, &payload);
    record.size = 0x1000; // claims more than the section holds
    let data = cia(
        0,
        &[0],
        &Sections {
            tik: vec![9u8; 256],
            tmd: tmd(1, 1, &[record]),
            contents: vec![payload],
            ..Sections::default()
        },
    );
    let error = parse(&data).unwrap_err();
    assert!(
        matches!(&error, Error::Corrupt(t) if t.contains("extends past the CIA")),
        "{error}"
    );
}

#[test]
fn a_content_index_the_bitarray_leaves_out() {
    let first = vec![1u8; 16];
    let second = vec![2u8; 16];
    let data = cia(
        0,
        &[0], // only content 0 present
        &Sections {
            tik: vec![9u8; 256],
            tmd: tmd(1, 1, &[rec(0xAA, 0, &first), rec(0xBB, 1, &second)]),
            contents: vec![first.clone(), second],
            ..Sections::default()
        },
    );
    let cia = parse(&data).unwrap();
    assert_eq!(cia.contents().len(), 1);
    assert_eq!(cia.contents()[0].id, 0xAA);

    let dir = tempfile::tempdir().unwrap();
    let path = write_cia(dir.path(), &data);
    let out = dir.path().join("out");
    let written = unpack(&path, &out, &UnpackOptions::default(), &mut |_| {}).unwrap();
    let contents: Vec<_> = written
        .iter()
        .filter(|file| matches!(file.part, Part::Content(_)))
        .collect();
    assert_eq!(contents.len(), 1);
    assert_eq!(std::fs::read(&contents[0].path).unwrap(), first);

    // The skipped content's bytes are still counted: the progress walk is
    // over the file, not over what was written.
    let mut consumed = 0;
    unpack(&path, &out, &UnpackOptions::default(), &mut |n| {
        consumed += n
    })
    .unwrap();
    assert_eq!(consumed, std::fs::metadata(&path).unwrap().len());
}

#[test]
fn the_bitarray_covers_every_content_index() {
    // The last index sits in the bitarray's last byte, at 0x201F.
    let payload = vec![3u8; 16];
    let data = cia(
        0,
        &[u16::MAX],
        &Sections {
            tik: vec![9u8; 256],
            tmd: tmd(1, 1, &[rec(0xCC, u16::MAX, &payload)]),
            contents: vec![payload],
            ..Sections::default()
        },
    );
    let cia = parse(&data).unwrap();
    assert_eq!(cia.contents().len(), 1);
    assert_eq!(cia.contents()[0].index, u16::MAX);
}

#[test]
fn a_repeated_content_index_keeps_the_last_record() {
    let first = vec![1u8; 16];
    let second = vec![2u8; 16];
    let data = cia(
        0,
        &[0],
        &Sections {
            tik: vec![9u8; 256],
            tmd: tmd(1, 1, &[rec(0xAA, 0, &first), rec(0xBB, 0, &second)]),
            contents: vec![first, second],
            ..Sections::default()
        },
    );
    let cia = parse(&data).unwrap();
    assert_eq!(cia.contents().len(), 1);
    assert_eq!(cia.contents()[0].id, 0xBB);
    assert_eq!(cia.contents()[0].size, 16);
}

#[test]
fn sections_start_where_the_alignment_says() {
    let payload = vec![3u8; 20];
    let data = cia(
        0,
        &[0],
        &Sections {
            certs: vec![0u8; 48],
            tik: vec![0u8; 40],
            tmd: tmd(1, 1, &[rec(5, 0, &payload)]),
            contents: vec![payload],
            footer: vec![0u8; 4],
        },
    );
    let cia = parse(&data).unwrap();
    // 0x2020 → align64 → 0x2040 certs(48) → align64 → 0x2080 tik(40) →
    // align64 → 0x20C0 tmd(2868) → align64 → 0x2C00 contents → footer.
    assert_eq!(cia.section(Section::Certificates), Some((0x2040, 48)));
    assert_eq!(cia.section(Section::Ticket), Some((0x2080, 40)));
    assert_eq!(
        cia.section(Section::TitleMetaData),
        Some((0x20C0, 384 + 2436 + 48))
    );
    assert_eq!(cia.contents()[0].offset, 0x2C00);
    // The 20-byte content rounds the running size to 32 before the footer.
    assert_eq!(cia.section(Section::Footer), Some((0x2C40, 4)));
}
