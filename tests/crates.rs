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
        "crates/rvz-rs/src/bin/rvzrs/ui.rs",
        "crates/wbfs-rs/src/bin/wbfsrs/ui.rs",
        "crates/xdelta-rs/src/bin/xdeltars/ui.rs",
        "crates/wux-rs/src/bin/wuxrs/ui.rs",
        "crates/xps-rs/src/bin/xpsrs/ui.rs",
    ] {
        assert!(read(copy) == ui, "{copy} differs from xsors's ui.rs");
    }
}

/// Crates embedded rather than published, each symlinked into the source of
/// the crates using it, by where each symlink sits.
const EMBEDDED: &[(&str, &[&str])] = &[(
    "crates/gcwii",
    &[
        "crates/rvz-rs/src/gcwii",
        "crates/rvz-rs/tests/common",
        "crates/wbfs-rs/src/gcwii",
        "crates/wbfs-rs/tests/common",
    ],
)];

#[test]
fn embedded_crates_are_symlinked_in() {
    for (embedded, links) in EMBEDDED {
        for link in *links {
            let target = std::fs::read_link(link).unwrap_or_else(|e| panic!("{link}: {e}"));
            let resolved = std::path::Path::new(link).parent().unwrap().join(&target);
            let wanted = std::path::Path::new(embedded).join(if link.ends_with("common") {
                "tests/common"
            } else {
                "src"
            });
            assert_eq!(
                std::fs::canonicalize(&resolved).unwrap(),
                std::fs::canonicalize(&wanted).unwrap(),
                "{link} should link to {}",
                wanted.display()
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
