//! Known answers: the fixture CIA in `tests/data`, checked against the DAT
//! entry it was built with — the same hashes oxyromon imports against, which
//! is what pins the offsets this crate computes.

use cia_rs::{Cia, CiaFormat, Part, UnpackOptions, unpack};

fn crc32(data: &[u8]) -> u32 {
    let mut crc = crc32fast::Hasher::new();
    crc.update(data);
    crc.finalize()
}

#[test]
fn header_and_title_metadata() {
    let cia = Cia::parse(&crate_path(), &mut |_| {}).unwrap();
    assert_eq!(cia.format(), CiaFormat::Normal);
    assert_eq!(cia.title_id(), 0x0004_0000_0F80_0100);
    assert_eq!(cia.title_version(), 2145);
    assert_eq!(cia.tmd_meta_size(), 2868);

    let contents = cia.contents();
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0].id, 0x7926_FC67);
    assert_eq!(contents[0].index, 0);
    assert_eq!(contents[0].size, 1_163_264);
    assert_eq!(
        contents[0].hash,
        hex_hash("74843282f5d54bbe8b5440197d34e9d333452bba90b7fc9a848fac33557c25d5")
    );

    // The DAT's rom entries: one content, plus the TMD with the CA and TMD
    // certificates appended — the very bytes the reference tool's plain-mode
    // extraction is defined by.
    let out = tempfile::tempdir().unwrap();
    let written = unpack(
        &crate_path(),
        out.path(),
        &UnpackOptions::default(),
        &mut |_| {},
    )
    .unwrap();
    let read = |part: &str| std::fs::read(out.path().join(part)).unwrap();

    let content = read("content.0000.7926fc67");
    assert_eq!(content.len(), 1_163_264);
    assert_eq!(crc32(&content), 0x062E_E486);

    let tmd = read("tmd");
    let certs = read("certs");
    assert_eq!(tmd.len(), 2868);
    assert_eq!(certs.len(), 2560);
    let mut appended = tmd.clone();
    appended.extend_from_slice(&certs[0x700..0xA00]);
    appended.extend_from_slice(&certs[0x000..0x400]);
    assert_eq!(crc32(&appended), 0xDB77_5B78);

    let parts: Vec<&str> = written
        .iter()
        .map(|file| match &file.part {
            Part::Certificates => "certs",
            Part::Ticket => "tik",
            Part::TitleMetaData => "tmd",
            Part::Content(_) => "content",
            Part::Footer => "footer",
        })
        .collect();
    assert_eq!(parts, ["certs", "tik", "tmd", "content", "footer"]);
}

#[test]
fn parse_reports_the_metadata_it_reads() {
    let mut consumed = 0;
    let cia = Cia::parse(&crate_path(), &mut |n| consumed += n).unwrap();
    let tmd = cia.section(cia_rs::Section::TitleMetaData).unwrap();
    assert_eq!(consumed, tmd.0 + u64::from(cia.tmd_meta_size()));
}

#[test]
fn unpack_reports_the_whole_file() {
    let length = std::fs::metadata(crate_path()).unwrap().len();
    let out = tempfile::tempdir().unwrap();
    let mut consumed = 0;
    unpack(
        &crate_path(),
        out.path(),
        &UnpackOptions::default(),
        &mut |n| consumed += n,
    )
    .unwrap();
    assert_eq!(consumed, length);
}

#[test]
fn options_leave_parts_behind() {
    let out = tempfile::tempdir().unwrap();
    let written = unpack(
        &crate_path(),
        out.path(),
        &UnpackOptions {
            ticket: false,
            footer: false,
        },
        &mut |_| {},
    )
    .unwrap();
    assert!(!out.path().join("tik").exists());
    assert!(!out.path().join("footer").exists());
    assert!(out.path().join("tmd").exists());
    assert!(
        written
            .iter()
            .all(|file| !matches!(file.part, Part::Ticket | Part::Footer))
    );
}

fn crate_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/Test Game (USA, Europe).cia")
}

fn hex_hash(hex: &str) -> [u8; 32] {
    let bytes: Vec<u8> = hex
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).unwrap();
            u8::from_str_radix(text, 16).unwrap()
        })
        .collect();
    bytes.try_into().unwrap()
}
