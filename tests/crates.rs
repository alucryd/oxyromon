//! The format crates' CLIs share one shell, copied into each: keep the copies
//! the same.

#[test]
fn every_cli_shares_one_ui() {
    let read = |path: &str| std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let ui = read("crates/xso-rs/src/bin/xsors/ui.rs");
    for copy in [
        "crates/chd-rs/src/bin/chdrs/ui.rs",
        "crates/cia-rs/src/bin/ciars/ui.rs",
        "crates/gdi-rs/src/bin/gdirs/ui.rs",
        "crates/nsz-rs/src/bin/nszrs/ui.rs",
        "crates/ore-rs/src/bin/orers/ui.rs",
        "crates/rvz-rs/src/bin/rvzrs/ui.rs",
        "crates/wbfs-rs/src/bin/wbfsrs/ui.rs",
        "crates/xdelta-rs/src/bin/xdeltars/ui.rs",
        "crates/wux-rs/src/bin/wuxrs/ui.rs",
        "crates/xps-rs/src/bin/xpsrs/ui.rs",
    ] {
        assert!(read(copy) == ui, "{copy} differs from xsors's ui.rs");
    }
}

/// Crates embedded rather than published, each copied into the source of the
/// crates using it, by where each copy sits: a symlink checks out on Windows as
/// a text file. What the tests share is not copied: it lives in
/// `crates/test-support`, used as a dev-dependency.
const EMBEDDED: &[(&str, &[&str])] = &[
    ("crates/cd", &["crates/chd-rs/src/cd"]),
    (
        "crates/gcwii",
        &["crates/rvz-rs/src/gcwii", "crates/wbfs-rs/src/gcwii"],
    ),
    ("crates/wiiu", &["crates/wux-rs/src/wiiu"]),
];

/// Every file under `dir`, by its path relative to `dir`, and its bytes.
fn files(dir: &std::path::Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
    let mut files = std::collections::BTreeMap::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(at) = dirs.pop() {
        for entry in std::fs::read_dir(&at).unwrap_or_else(|e| panic!("{}: {e}", at.display())) {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
            } else {
                let bytes = std::fs::read(&path).unwrap();
                files.insert(path.strip_prefix(dir).unwrap().to_path_buf(), bytes);
            }
        }
    }
    files
}

#[test]
fn embedded_crates_are_copied_in() {
    for (embedded, copies) in EMBEDDED {
        for copy in *copies {
            let source = std::path::Path::new(embedded).join("src");
            // Reading follows a symlink, which would then pass
            assert!(
                !std::path::Path::new(copy).is_symlink(),
                "{copy} is a symlink: make it a copy of {}",
                source.display()
            );
            assert!(
                files(&source) == files(std::path::Path::new(copy)),
                "{copy} differs from {}: replace it with a fresh copy",
                source.display()
            );
        }
    }
}

#[test]
fn embedded_crates_never_name_their_root() {
    // Embedded, their root is the embedding crate's: `crate::` would not
    // reach them there
    for (embedded, _) in EMBEDDED {
        let src = std::path::Path::new(embedded).join("src");
        for entry in std::fs::read_dir(&src).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            for (i, line) in text.lines().enumerate() {
                let code = line.split("//").next().unwrap();
                assert!(
                    !code.contains("crate::"),
                    "{}:{}: use `super::`, not `crate::`",
                    path.display(),
                    i + 1
                );
            }
        }
    }
}
