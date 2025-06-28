use async_once_cell::OnceCell;
use infer::{Infer, Type};
use simple_error::SimpleResult;
use std::path::Path;

pub const BIN_EXTENSION: &str = "bin";
pub const BPS_EXTENSION: &str = "bps";
pub const CHD_EXTENSION: &str = "chd";
pub const CIA_EXTENSION: &str = "cia";
pub const CSO_EXTENSION: &str = "cso";
pub const CUE_EXTENSION: &str = "cue";
pub const DAT_EXTENSION: &str = "dat";
pub const GZ_EXTENSION: &str = "gz";
pub const IPS_EXTENSION: &str = "ips";
pub const IRD_EXTENSION: &str = "ird";
pub const ISO_EXTENSION: &str = "iso";
pub const M3U_EXTENSION: &str = "m3u";
pub const NSP_EXTENSION: &str = "nsp";
pub const NSZ_EXTENSION: &str = "nsz";
pub const PKG_EXTENSION: &str = "pkg";
pub const PUP_EXTENSION: &str = "pup";
pub const RAP_EXTENSION: &str = "rap";
pub const RDSK_EXTENSION: &str = "rdsk";
pub const RIFF_EXTENSION: &str = "riff";
pub const RVZ_EXTENSION: &str = "rvz";
pub const SEVENZIP_EXTENSION: &str = "7z";
pub const WBFS_EXTENSION: &str = "wbfs";
pub const XDELTA_EXTENSION: &str = "xdelta";
pub const ZIP_EXTENSION: &str = "zip";
pub const ZSO_EXTENSION: &str = "zso";

pub static ARCHIVE_EXTENSIONS: [&str; 2] = [SEVENZIP_EXTENSION, ZIP_EXTENSION];
pub static PS3_EXTENSIONS: [&str; 3] = [PKG_EXTENSION, PUP_EXTENSION, RAP_EXTENSION];

pub static NON_ORIGINAL_EXTENSIONS: &[&str] = &[
    CHD_EXTENSION,
    CSO_EXTENSION,
    NSZ_EXTENSION,
    RVZ_EXTENSION,
    ZSO_EXTENSION,
    SEVENZIP_EXTENSION,
    ZIP_EXTENSION,
];

static MATCHER: OnceCell<Infer> = OnceCell::new();

fn bps_matcher(buf: &[u8]) -> bool {
    buf.len() >= 4 && buf[0] == 0x42 && buf[1] == 0x50 && buf[2] == 0x53 && buf[3] == 0x31
}

fn chd_matcher(buf: &[u8]) -> bool {
    buf.len() >= 8
        && buf[0] == 0x4D
        && buf[1] == 0x43
        && buf[2] == 0x6F
        && buf[3] == 0x6D
        && buf[4] == 0x70
        && buf[5] == 0x72
        && buf[6] == 0x48
        && buf[7] == 0x44
}

fn cso_matcher(buf: &[u8]) -> bool {
    buf.len() >= 4 && buf[0] == 0x43 && buf[1] == 0x49 && buf[2] == 0x53 && buf[3] == 0x4F
}

fn ips_matcher(buf: &[u8]) -> bool {
    buf.len() >= 5
        && buf[0] == 0x50
        && buf[1] == 0x41
        && buf[2] == 0x54
        && buf[3] == 0x43
        && buf[4] == 0x48
}

fn ird_matcher(buf: &[u8]) -> bool {
    buf.len() >= 4 && buf[0] == 0x33 && buf[1] == 0x49 && buf[2] == 0x52 && buf[3] == 0x44
}

fn rdsk_matcher(buf: &[u8]) -> bool {
    buf.len() >= 4 && buf[0] == 0x52 && buf[1] == 0x44 && buf[2] == 0x53 && buf[3] == 0x4B
}

fn riff_matcher(buf: &[u8]) -> bool {
    buf.len() >= 4 && buf[0] == 0x52 && buf[1] == 0x49 && buf[2] == 0x46 && buf[3] == 0x46
}

fn rvz_matcher(buf: &[u8]) -> bool {
    buf.len() >= 4 && buf[0] == 0x52 && buf[1] == 0x56 && buf[2] == 0x5A && buf[3] == 0x01
}

fn xdelta_matcher(buf: &[u8]) -> bool {
    buf.len() >= 3 && buf[0] == 0xD6 && buf[1] == 0xC3 && buf[2] == 0xC4
}

fn zso_matcher(buf: &[u8]) -> bool {
    buf.len() >= 4 && buf[0] == 0x5A && buf[1] == 0x49 && buf[2] == 0x53 && buf[3] == 0x4F
}

async fn init_matcher() -> Infer {
    let mut matcher = Infer::new();
    matcher.add("application/x-bps", BPS_EXTENSION, bps_matcher);
    matcher.add("application/x-chd", CHD_EXTENSION, chd_matcher);
    matcher.add("application/x-cso", CSO_EXTENSION, cso_matcher);
    matcher.add("application/x-ips", IPS_EXTENSION, ips_matcher);
    matcher.add("application/x-ird", IRD_EXTENSION, ird_matcher);
    matcher.add("application/x-rdsk", RDSK_EXTENSION, rdsk_matcher);
    matcher.add("application/x-riff", RIFF_EXTENSION, riff_matcher);
    matcher.add("application/x-rvz", RVZ_EXTENSION, rvz_matcher);
    matcher.add("application/x-xdelta", XDELTA_EXTENSION, xdelta_matcher);
    matcher.add("application/x-zso", ZSO_EXTENSION, zso_matcher);
    matcher
}

pub async fn get_mimetype<P: AsRef<Path>>(path: &P) -> SimpleResult<Option<Type>> {
    let matcher = MATCHER.get_or_init(init_matcher()).await;
    Ok(try_with!(
        matcher.get_from_path(path),
        "Failed to infer MIME type"
    ))
}
