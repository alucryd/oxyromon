//! Binary format readers/writers: NCA header, NCZ tables, PFS0/NSP, CNMT.

use std::io::{self, Read};

pub mod cnmt;
pub mod nca;
pub mod ncz;
pub mod pfs0;

/// Read exactly `len` bytes. The buffer grows as data actually arrives, so a
/// bogus length from an untrusted header fails with `UnexpectedEof` instead of
/// attempting a huge up-front allocation.
pub(crate) fn read_vec<R: Read>(r: &mut R, len: u64) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    r.by_ref().take(len).read_to_end(&mut buf)?;
    if (buf.len() as u64) < len {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "truncated data",
        ));
    }
    Ok(buf)
}
