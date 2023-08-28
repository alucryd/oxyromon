use std::env;
use std::fs;
use std::process::Command;

fn main() {
    if cfg!(feature = "server") && env::var("SKIP_YARN").unwrap_or_default() != "true" {
        Command::new("yarn")
            .arg("install")
            .arg("--frozen-lockfile")
            .output()
            .expect("failed to run yarn install");
        Command::new("yarn")
            .arg("build")
            .output()
            .expect("failed to run yarn build");
        fs::remove_dir_all(".svelte-kit").ok();
        fs::remove_dir_all("node_modules").ok();
    }
}
