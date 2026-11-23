use super::*;
use std::path::PathBuf;

#[test]
fn test() {
    let nsp = PathBuf::from("x.nsp");
    let nsz = PathBuf::from("x.nsz");

    // the casts are gated on the extension alone
    assert!(CommonRomfile::from_path(&nsp).unwrap().as_nsp().is_ok());
    assert!(CommonRomfile::from_path(&nsz).unwrap().as_nsz().is_ok());
    assert!(CommonRomfile::from_path(&nsz).unwrap().as_nsp().is_err());
    assert!(CommonRomfile::from_path(&nsp).unwrap().as_nsz().is_err());
}
