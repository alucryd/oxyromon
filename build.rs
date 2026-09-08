use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    // Rebuild the embedded web UI (Leptos + Trunk) when the `server` feature is
    // enabled. Set SKIP_TRUNK=true to skip (e.g. when assets are prebuilt).
    if cfg!(feature = "server") && env::var("SKIP_TRUNK").unwrap_or_default() != "true" {
        // `frontend` is a package in its own right, and cargo leaves nested
        // packages out of the tarball it publishes, so it is simply absent when
        // building from crates.io. Check for it before shelling out: `Command`
        // reports a missing working directory exactly as it reports a missing
        // binary, so without this the failure below blames trunk and sends
        // people off to install one they already have.
        assert!(
            Path::new("frontend").exists(),
            "the `server` feature builds the web UI from `frontend/`, which is \
             not part of the published crate. Build from a git checkout of \
             https://github.com/alucryd/oxyromon, or set SKIP_TRUNK=true if \
             `target/assets` already holds a built UI."
        );

        let status = Command::new("trunk")
            .arg("build")
            .arg("--release")
            .current_dir("frontend")
            .status()
            .expect(
                "failed to run `trunk build`; install trunk (https://trunkrs.dev), \
                 or set SKIP_TRUNK=true",
            );
        assert!(status.success(), "`trunk build` failed");
    }

    println!("cargo:rerun-if-changed=frontend/src");
    println!("cargo:rerun-if-changed=frontend/index.html");
    println!("cargo:rerun-if-changed=frontend/styles.css");
    println!("cargo:rerun-if-changed=frontend/Trunk.toml");
    println!("cargo:rerun-if-changed=frontend/Cargo.toml");
}
