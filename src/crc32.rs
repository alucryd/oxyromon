use crc32fast::Hasher;
use digest::generic_array::typenum::U4;
use digest::generic_array::GenericArray;
use digest::OutputSizeUser;
use digest::{FixedOutput, HashMarker, Reset, Update};
use std::io::Write;

#[derive(Clone, Default)]
pub struct Crc32 {
    hasher: Hasher,
}

impl Crc32 {
    pub fn new() -> Self {
        Self {
            hasher: Hasher::new(),
        }
    }
}

impl HashMarker for Crc32 {}

impl OutputSizeUser for Crc32 {
    type OutputSize = U4;
}

impl FixedOutput for Crc32 {
    fn finalize_into(self, out: &mut GenericArray<u8, U4>) {
        out.copy_from_slice(&self.hasher.finalize().to_be_bytes());
    }
}

impl Update for Crc32 {
    fn update(&mut self, data: &[u8]) {
        self.hasher.update(data);
    }
}

impl Reset for Crc32 {
    fn reset(&mut self) {
        self.hasher.reset();
    }
}

impl Write for Crc32 {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Update::update(self, buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
