use std::env;
use std::fs;
use std::process::Command;

fn main() {
    if cfg!(feature = "server") && env::var("SKIP_PNPM").unwrap_or_default() != "true" {
        Command::new("pnpm")
            .arg("install")
            .arg("--frozen-lockfile")
            .output()
            .expect("failed to run pnpm install");
        Command::new("pnpm")
            .arg("build")
            .output()
            .expect("failed to run pnpm build");
        fs::remove_dir_all(".svelte-kit").ok();
        fs::remove_dir_all("node_modules").ok();
    }
}
