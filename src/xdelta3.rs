use super::SimpleResult;
use lazy_static::lazy_static;
use regex::Regex;
use tokio::process::Command;

const XDELTA3: &str = "xdelta3";

lazy_static! {
    static ref VERSION_REGEX: Regex = Regex::new(r"\d+\.\d+\.\d+").unwrap();
}

pub async fn get_version() -> SimpleResult<String> {
    let output = try_with!(
        Command::new(XDELTA3).arg("-V").output().await,
        "Failed to spawn xdelta3"
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    let version = stderr
        .lines()
        .next()
        .and_then(|line| VERSION_REGEX.find(line))
        .map(|version| version.as_str().to_string())
        .unwrap_or(String::from("unknown"));

    Ok(version)
}
