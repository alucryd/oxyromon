use super::model::Header;
use crc32fast::Hasher;
use digest::generic_array::typenum::{U4, U64};
use digest::generic_array::GenericArray;
use digest::Digest;
use digest::{BlockInput, FixedOutputDirty, Reset, Update};
use std::error::Error;
use std::fs;
use std::io;
use std::io::prelude::*;
use std::path::PathBuf;

#[derive(Clone, Default)]
struct Crc32 {
    hasher: Hasher,
}

impl Crc32 {
    pub fn new() -> Self {
        Self {
            hasher: Hasher::new(),
        }
    }
}

impl FixedOutputDirty for Crc32 {
    type OutputSize = U4;

    fn finalize_into_dirty(&mut self, out: &mut GenericArray<u8, U4>) {
        out.copy_from_slice(&self.hasher.to_owned().finalize().to_be_bytes());
    }
}

impl BlockInput for Crc32 {
    type BlockSize = U64;
}

impl Update for Crc32 {
    fn update(&mut self, input: impl AsRef<[u8]>) {
        self.hasher.update(input.as_ref());
    }
}

impl Reset for Crc32 {
    fn reset(&mut self) {
        self.hasher.reset();
    }
}

impl io::Write for Crc32 {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Update::update(self, buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub fn get_file_size_and_crc(
    file_path: &PathBuf,
    header: &Option<Header>,
) -> Result<(u64, String), Box<dyn Error>> {
    println!("Scanning {:?}", file_path.file_name().unwrap());
    let mut f = fs::File::open(&file_path)?;
    let mut size = f.metadata().unwrap().len();

    // extract a potential header, revert if none is found
    if header.is_some() {
        let header = header.as_ref().unwrap();

        let mut buffer: Vec<u8> = Vec::with_capacity(header.size as usize);
        (&mut f).take(header.size as u64).read_to_end(&mut buffer)?;
        let start_byte = header.start_byte as usize;
        let hex_values: Vec<String> = buffer[start_byte..].iter().map(|b| format!("{:x}", b)).collect();
        let hex_value = hex_values.join("").to_uppercase();

        if hex_value.starts_with(&header.hex_value.to_uppercase()) {
            size -= header.size as u64;
        } else {
            f.seek(std::io::SeekFrom::Start(0))?;
        }
    }

    // compute the checksum
    let mut digest = Crc32::new();
    io::copy(&mut f, &mut digest)?;
    let crc = format!("{:08x}", digest.finalize());
    Ok((size, crc))
}
