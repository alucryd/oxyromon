use std::env;
use std::process::Command;

fn main() {
    // Rebuild the embedded web UI (Leptos + Trunk) when the `server` feature is
    // enabled. Set SKIP_TRUNK=true to skip (e.g. when assets are prebuilt).
    if cfg!(feature = "server") && env::var("SKIP_TRUNK").unwrap_or_default() != "true" {
        let status = Command::new("trunk")
            .arg("build")
            .arg("--release")
            .current_dir("frontend")
            .status()
            .expect(
                "failed to run `trunk build`; install trunk (https://trunkrs.dev) and the \
                 tailwindcss standalone CLI, or set SKIP_TRUNK=true",
            );
        assert!(status.success(), "`trunk build` failed");
    }

    println!("cargo:rerun-if-changed=frontend/src");
    println!("cargo:rerun-if-changed=frontend/index.html");
    println!("cargo:rerun-if-changed=frontend/input.css");
    println!("cargo:rerun-if-changed=frontend/Trunk.toml");
    println!("cargo:rerun-if-changed=frontend/Cargo.toml");
}
