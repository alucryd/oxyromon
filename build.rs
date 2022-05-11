use std::env;
use std::fs;
use std::process::Command;

fn main() {
    let skip_yarn = env::var("SKIP_YARN").unwrap_or_default() == "true";
    let skip_yarn_cleanup = env::var("SKIP_YARN_CLEANUP").unwrap_or(String::from("true")) == "true";
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
        if !skip_yarn_cleanup {
            fs::remove_dir_all(".svelte-kit").expect("failed to delete .svelte-kit");
            fs::remove_dir_all("node_modules").expect("failed to delete node_modules");
        }
    }
}
