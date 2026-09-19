//! LZ4 block helpers.
//!
//! Compression goes straight to liblz4 so the output matches what maxcso
//! writes. Decompression is done here instead, because a CSO/ZSO block's stored
//! span is `next_offset - offset`, which includes the alignment padding that
//! `index_shift > 0` inserts after every block. liblz4's exact-length decoder
//! rejects that trailing garbage and `lz4_flex` has no partial mode at all, so
//! we decode with "stop when the output buffer is full" semantics -- the same
//! thing `LZ4_decompress_safe_partial` does.

use crate::error::{Error, Result};

/// Shortest match the format can encode.
const MINMATCH: usize = 4;

/// Decompress an LZ4 block into `dst`, stopping as soon as `dst` is full and
/// ignoring any trailing bytes.
///
/// Returns the number of bytes written. The caller decides whether a short fill
/// is an error.
pub fn decompress_bounded(src: &[u8], dst: &mut [u8]) -> Result<usize> {
    let mut ip = 0usize;
    let mut op = 0usize;

    while op < dst.len() {
        let Some(&token) = src.get(ip) else {
            return Err(Error::Corrupt("lz4 block ended before the output was full".into()));
        };
        ip += 1;

        // Literal run length: a 15 nibble means "keep adding following bytes".
        let mut lit_len = (token >> 4) as usize;
        if lit_len == 15 {
            loop {
                let Some(&b) = src.get(ip) else {
                    return Err(Error::Corrupt("lz4 literal length ran off the end of the block".into()));
                };
                ip += 1;
                lit_len = lit_len
                    .checked_add(b as usize)
                    .ok_or_else(|| Error::Corrupt("lz4 literal length overflow".into()))?;
                if b != 255 {
                    break;
                }
            }
        }

        // Only the bytes we actually need have to be present; the rest of the run
        // is cut off by the output limit.
        let take = lit_len.min(dst.len() - op);
        let Some(lits) = src.get(ip..ip + take) else {
            return Err(Error::Corrupt("lz4 literals ran off the end of the block".into()));
        };
        dst[op..op + take].copy_from_slice(lits);
        op += take;
        ip += lit_len;

        if op >= dst.len() {
            break;
        }

        // A block's final sequence is literals only, so an exhausted input here
        // just means the output limit cut the stream short.
        let Some(offset_bytes) = src.get(ip..ip + 2) else {
            break;
        };
        ip += 2;

        let offset = u16::from_le_bytes([offset_bytes[0], offset_bytes[1]]) as usize;
        if offset == 0 || offset > op {
            return Err(Error::Corrupt(format!(
                "lz4 match offset {offset} points outside the decoded data"
            )));
        }

        let mut match_len = (token & 0xF) as usize + MINMATCH;
        if (token & 0xF) == 15 {
            loop {
                let Some(&b) = src.get(ip) else {
                    return Err(Error::Corrupt("lz4 match length ran off the end of the block".into()));
                };
                ip += 1;
                match_len = match_len
                    .checked_add(b as usize)
                    .ok_or_else(|| Error::Corrupt("lz4 match length overflow".into()))?;
                if b != 255 {
                    break;
                }
            }
        }

        let take = match_len.min(dst.len() - op);
        let start = op - offset;
        // Copy at most `offset` bytes per pass so overlapping matches (run
        // lengths longer than the distance) expand correctly.
        let mut done = 0;
        while done < take {
            let chunk = offset.min(take - done);
            dst.copy_within(start + done..start + done + chunk, op + done);
            done += chunk;
        }
        op += take;
    }

    Ok(op)
}

/// Compress one block with the fast LZ4 compressor.
///
/// # Safety (internal)
/// `dst` must be at least `compress_bound(src.len())` bytes long.
pub fn compress_default(src: &[u8], dst: &mut [u8]) -> Option<usize> {
    // SAFETY: the caller (method::compress_block) allocates scratch via
    // scratch_size(), which is >= compress_bound(src.len()).
    let n = unsafe {
        lz4_sys::LZ4_compress_default(
            src.as_ptr().cast::<std::ffi::c_char>(),
            dst.as_mut_ptr().cast::<std::ffi::c_char>(),
            src.len() as i32,
            dst.len() as i32,
        )
    };
    if n > 0 {
        Some(n as usize)
    } else {
        None
    }
}

/// Compress one block with LZ4 HC at `level` (1..=16).
///
/// # Safety (internal)
/// `dst` must be at least `compress_bound(src.len())` bytes long.
pub fn compress_hc(src: &[u8], dst: &mut [u8], level: i32) -> Option<usize> {
    // SAFETY: the caller (method::compress_block) allocates scratch via
    // scratch_size(), which is >= compress_bound(src.len()).
    let n = unsafe {
        lz4_sys::LZ4_compress_HC(
            src.as_ptr().cast::<std::ffi::c_char>(),
            dst.as_mut_ptr().cast::<std::ffi::c_char>(),
            src.len() as i32,
            dst.len() as i32,
            level,
        )
    };
    if n > 0 {
        Some(n as usize)
    } else {
        None
    }
}

/// Upper bound on the compressed size of `len` input bytes.
pub fn compress_bound(len: usize) -> usize {
    (unsafe { lz4_sys::LZ4_compressBound(len as i32) }).max(0) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples() -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        out.push(vec![0u8; 2048]);
        out.push(vec![0xABu8; 2048]);
        let mut mixed = vec![0u8; 2048];
        for (i, b) in mixed.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        out.push(mixed);
        let mut text = Vec::new();
        while text.len() < 2048 {
            text.extend_from_slice(b"The quick brown fox jumps over the lazy dog. ");
        }
        text.truncate(2048);
        out.push(text);
        // Incompressible: lz4 will store it nearly verbatim.
        let mut noise = Vec::with_capacity(2048);
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        while noise.len() < 2048 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            noise.push((state >> 24) as u8);
        }
        out.push(noise);
        out
    }

    #[test]
    fn roundtrip_through_liblz4() {
        for src in samples() {
            for level in [4, 7, 10, 13, 16] {
                let mut buf = vec![0u8; compress_bound(src.len())];
                let n = compress_hc(&src, &mut buf, level).unwrap();
                let mut back = vec![0u8; src.len()];
                let got = decompress_bounded(&buf[..n], &mut back).unwrap();
                assert_eq!(got, src.len(), "level {level} filled {got} of {}", src.len());
                assert_eq!(back, src, "level {level} mismatch");
            }
            let mut buf = vec![0u8; compress_bound(src.len())];
            let n = compress_default(&src, &mut buf).unwrap();
            let mut back = vec![0u8; src.len()];
            let got = decompress_bounded(&buf[..n], &mut back).unwrap();
            assert_eq!(got, src.len());
            assert_eq!(back, src);
        }
    }

    #[test]
    fn ignores_trailing_alignment_padding() {
        let src = samples()[2].clone();
        let mut buf = vec![0u8; compress_bound(src.len())];
        let n = compress_hc(&src, &mut buf, 16).unwrap();

        let mut padded = buf[..n].to_vec();
        padded.extend_from_slice(&[0u8; 4096]);

        let mut back = vec![0u8; src.len()];
        let got = decompress_bounded(&padded, &mut back).unwrap();
        assert_eq!(got, src.len());
        assert_eq!(back, src);
    }

    #[test]
    fn stops_at_a_truncated_output_limit() {
        let src = samples()[3].clone();
        let mut buf = vec![0u8; compress_bound(src.len())];
        let n = compress_hc(&src, &mut buf, 16).unwrap();

        // What the last block of a file looks like: full stream, smaller dst.
        let mut back = vec![0u8; 1000];
        let got = decompress_bounded(&buf[..n], &mut back).unwrap();
        assert_eq!(got, 1000);
        assert_eq!(back, &src[..1000]);
    }

    #[test]
    fn rejects_a_truncated_input() {
        let src = samples()[2].clone();
        let mut buf = vec![0u8; compress_bound(src.len())];
        let n = compress_hc(&src, &mut buf, 16).unwrap();
        let mut back = vec![0u8; src.len()];
        assert!(decompress_bounded(&buf[..n / 2], &mut back).is_err());
    }

    #[test]
    fn rejects_a_zero_offset_match() {
        // token: 4 literals then a match with offset 0.
        let bad = [0x40u8, b'a', b'b', b'c', b'd', 0x00, 0x00];
        let mut back = vec![0u8; 64];
        assert!(decompress_bounded(&bad, &mut back).is_err());
    }

    #[test]
    fn expands_a_run_longer_than_its_distance() {
        // One literal 'Z', then a distance-1 match of 300 bytes, which is what
        // exercises the overlapping-copy loop.
        //   token 0x1F: 1 literal, match nibble 15 (length follows)
        //   'Z', offset 0x0001 little-endian, then 296 = 255 + 41
        let encoded = [0x1Fu8, b'Z', 0x01, 0x00, 255, 41];
        let mut back = vec![0u8; 301];
        let got = decompress_bounded(&encoded, &mut back).unwrap();
        assert_eq!(got, 301);
        assert!(back.iter().all(|b| *b == b'Z'));
    }

    #[test]
    fn long_literal_run_needs_its_length_extensions() {
        // 300 literals: nibble 15 plus 255 + 45.
        let mut encoded = vec![0xF0u8, 255, 45];
        encoded.extend((0..300u32).map(|i| (i as u8).wrapping_add(7)));
        let mut back = vec![0u8; 300];
        let got = decompress_bounded(&encoded, &mut back).unwrap();
        assert_eq!(got, 300);
        assert_eq!(back, encoded[3..]);
    }
}

