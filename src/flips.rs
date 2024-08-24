use super::SimpleResult;
use tokio::process::Command;

const FLIPS: &str = "flips";

pub async fn get_version() -> SimpleResult<String> {
    let output = try_with!(
        Command::new(FLIPS).arg("-v").output().await,
        "Failed to spawn flips"
    );

    // flips doesn't advertise any version
    String::from_utf8(output.stderr).unwrap();
    let version = String::from("unknown");

    Ok(version)
}
