use super::*;

#[test]
fn test_valid() {
    assert!(ips_matcher(&[0x50, 0x41, 0x54, 0x43, 0x48]));
}

#[test]
fn test_invalid() {
    assert!(!ips_matcher(&[0x00, 0x00, 0x00, 0x00, 0x00]));
}

#[test]
fn test_too_short() {
    assert!(!ips_matcher(&[0x50, 0x41, 0x54, 0x43]));
}
