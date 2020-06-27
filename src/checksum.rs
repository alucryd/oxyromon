use super::model::Header;
use crc::{crc32, Hasher32};
use std::convert::TryFrom;
use std::error::Error;
use std::fs;
use std::io::prelude::*;
use std::path::PathBuf;

pub fn get_file_size_and_crc(
    file_path: &PathBuf,
    header: &Option<Header>,
) -> Result<(u64, String), Box<dyn Error>> {
    let mut f = fs::File::open(&file_path)?;
    let mut size = f.metadata().unwrap().len();

    // extract a potential header, revert if none is found
    if header.is_some() {
        let header = header.as_ref().unwrap();

        let mut buffer: Vec<u8> = Vec::with_capacity(header.size as usize);
        (&mut f).take(header.size as u64).read_to_end(&mut buffer)?;
        let start = header.start as usize;
        let hex_values: Vec<String> = buffer[start..].iter().map(|b| format!("{:x}", b)).collect();
        let hex_value = hex_values.join("").to_uppercase();

        if hex_value.starts_with(&header.hex_value.to_uppercase()) {
            size -= header.size as u64;
        } else {
            f.seek(std::io::SeekFrom::Start(0))?;
        }
    }

    // read our file in 4k chunks
    let mut digest = crc32::Digest::new(crc32::IEEE);
    let mut buffer = [0; 4096];
    loop {
        let n = f.read(&mut buffer[..])?;
        if n == 0 {
            break;
        }
        digest.write(&mut buffer[..n]);
    }

    let crc = format!("{:08x}", digest.sum32());
    Ok((size, crc))
}

pub fn get_chd_crcs(file_path: &PathBuf, sizes: &Vec<u64>) -> Result<Vec<String>, Box<dyn Error>> {
    let mut f = fs::File::open(&file_path)?;
    let size = f.metadata().unwrap().len();

    if size != sizes.iter().sum() {
        println!("Size(s) don't match");
        bail!("Size(s) don't match");
    }

    let mut crcs: Vec<String> = Vec::new();
    const BUFFER_SIZE: usize = 4096;

    for size in sizes {
        let mut digest = crc32::Digest::new(crc32::IEEE);
        let mut buffer = [0; BUFFER_SIZE];
        let mut consumed_bytes: usize = 0;

        // read 4k chunks until near the end
        loop {
            let n = f.read(&mut buffer[..])?;
            digest.write(&mut buffer[..n]);
            consumed_bytes += n;
            if (consumed_bytes as u64) + (BUFFER_SIZE as u64) >= *size {
                break;
            }
        }
        // read the exact remaining amount
        let remaining_bytes = size - consumed_bytes as u64;
        let remaining_bytes_usize = usize::try_from(remaining_bytes).unwrap();
        let mut buffer: Vec<u8> = Vec::with_capacity(remaining_bytes_usize);
        (&mut f).take(remaining_bytes).read_to_end(&mut buffer)?;
        digest.write(&mut buffer);

        crcs.push(format!("{:08x}", digest.sum32()));
    }
    Ok(crcs)
}
