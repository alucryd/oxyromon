//! The format crates' CLIs share one shell, copied into each: keep the copies
//! the same.

#[test]
fn every_cli_shares_one_ui() {
    let read = |path: &str| std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let ui = read("crates/xso-rs/src/bin/xsors/ui.rs");
    for copy in [
        "crates/gdi-rs/src/bin/gdirs/ui.rs",
        "crates/nsz-rs/src/bin/nszrs/ui.rs",
        "crates/xdelta-rs/src/bin/xdeltars/ui.rs",
    ] {
        assert!(read(copy) == ui, "{copy} differs from xsors's ui.rs");
    }
}
