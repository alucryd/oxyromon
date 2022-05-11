use std::env;
use std::fs;
use std::process::Command;

fn main() {
    let skip_yarn = env::var("SKIP_YARN").unwrap_or_default() == "true";
    if !skip_yarn {
        Command::new("yarn")
            .arg("install")
            .arg("--frozen-lockfile")
            .output()
            .expect("failed to run yarn install");
        Command::new("yarn")
            .arg("build")
            .output()
            .expect("failed to run yarn build");
        fs::remove_dir_all(".svelte-kit").expect("failed to delete .svelte-kit");
        fs::remove_dir_all("node_modules").expect("failed to delete node_modules");
    }
}
