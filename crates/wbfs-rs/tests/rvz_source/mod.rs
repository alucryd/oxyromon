//! An RVZ as a disc to write to WBFS, as oxyromon reads one: through rvz-rs,
//! the title keys it keeps standing in for the common key.

use wbfs_rs::{Error, Key, Result, Source};

pub struct Rvz(pub rvz_rs::Reader);

fn convert(error: rvz_rs::Error) -> Error {
    Error::Io(std::io::Error::other(error))
}

impl Source for Rvz {
    fn size(&self) -> u64 {
        self.0.size()
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.0.read_at(offset, buf).map_err(convert)
    }

    fn title_key(&self, data_offset: u64) -> Option<Key> {
        self.0
            .title_keys()
            .find(|(offset, _)| *offset == data_offset)
            .map(|(_, key)| key)
    }
}

pub fn open(path: &std::path::Path) -> Rvz {
    Rvz(rvz_rs::Reader::open(path).unwrap())
}
