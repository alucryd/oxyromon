//! A disc's file system table, as far as RVZ needs it: where each file lies.
//! Validated as Dolphin's `FileSystemGCWii` validates it.

use std::collections::BTreeMap;

const ENTRY_SIZE: usize = 12;

/// What no file system should exceed: more than a Wii's whole RAM.
pub const MAX_SIZE: u64 = 128 * 1024 * 1024;

pub struct FileSystem {
    fst: Vec<u8>,
    /// File offsets are stored shifted right by this much: 2 on Wii, 0 on
    /// GameCube.
    shift: u32,
    entries: u32,
    /// File end offset to entry index, the first file to end there.
    by_end: BTreeMap<u64, u32>,
}

impl FileSystem {
    /// Parse `fst`, or `None` if it is not a valid file system table.
    pub fn new(fst: Vec<u8>, shift: u32) -> Option<Self> {
        if fst.len() < ENTRY_SIZE {
            return None;
        }
        let mut fs = Self {
            fst,
            shift,
            entries: 0,
            by_end: BTreeMap::new(),
        };
        if !fs.is_directory(0) {
            return None;
        }
        fs.entries = fs.get(0, 2);
        if ENTRY_SIZE as u64 * fs.entries as u64 > fs.fst.len() as u64 {
            return None;
        }
        if fs.fst.last() != Some(&0) {
            return None;
        }
        if !fs.is_valid(0, 0) {
            return None;
        }
        for index in 0..fs.entries {
            if !fs.is_directory(index) {
                let size = fs.get(index, 2);
                if size != 0 {
                    fs.by_end
                        .entry(fs.offset(index) + size as u64)
                        .or_insert(index);
                }
            }
        }
        Some(fs)
    }

    fn get(&self, index: u32, property: usize) -> u32 {
        let at = ENTRY_SIZE * index as usize + 4 * property;
        u32::from_be_bytes(self.fst[at..at + 4].try_into().unwrap())
    }

    fn is_directory(&self, index: u32) -> bool {
        self.get(index, 0) & 0xFF000000 != 0
    }

    fn offset(&self, index: u32) -> u64 {
        (self.get(index, 1) as u64) << self.shift
    }

    fn next_index(&self, index: u32) -> u32 {
        if self.is_directory(index) {
            self.get(index, 2)
        } else {
            index + 1
        }
    }

    fn is_valid(&self, index: u32, parent: u32) -> bool {
        let name_offset =
            ENTRY_SIZE as u64 * self.entries as u64 + (self.get(index, 0) & 0xFFFFFF) as u64;
        if name_offset >= self.fst.len() as u64 {
            return false;
        }
        if self.is_directory(index) {
            if self.get(index, 1) != parent {
                return false;
            }
            let size = self.get(index, 2);
            if size <= index || size > self.get(parent, 2) {
                return false;
            }
            let mut child = index + 1;
            while child < size {
                if !self.is_valid(child, index) {
                    return false;
                }
                child = self.next_index(child);
            }
        }
        true
    }

    /// Every file, as its offset and size.
    pub fn files(&self) -> impl Iterator<Item = (u64, u64)> + '_ {
        (1..self.entries)
            .filter(|&index| !self.is_directory(index))
            .map(|index| (self.offset(index), self.get(index, 2) as u64))
    }

    /// The file `offset` falls in, as its offset and size.
    pub fn file_at(&self, offset: u64) -> Option<(u64, u64)> {
        let (_, &index) = self
            .by_end
            .range((
                std::ops::Bound::Excluded(offset),
                std::ops::Bound::Unbounded,
            ))
            .next()?;
        let start = self.offset(index);
        (start <= offset).then(|| (start, self.get(index, 2) as u64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Root, a file, a directory holding a file, and their names.
    fn fst() -> Vec<u8> {
        let mut fst = Vec::new();
        let entry = |fst: &mut Vec<u8>, name: u32, offset: u32, size: u32| {
            fst.extend(name.to_be_bytes());
            fst.extend(offset.to_be_bytes());
            fst.extend(size.to_be_bytes());
        };
        entry(&mut fst, 0x0100_0000, 0, 4);
        entry(&mut fst, 0, 0x1000, 0x20);
        entry(&mut fst, 0x0100_0002, 0, 4);
        entry(&mut fst, 4, 0x2000, 0x10);
        fst.extend(b"a\0d\0b\0");
        fst
    }

    #[test]
    fn finds_files() {
        let fs = FileSystem::new(fst(), 0).unwrap();
        assert_eq!(
            fs.files().collect::<Vec<_>>(),
            [(0x1000, 0x20), (0x2000, 0x10)]
        );
        assert_eq!(fs.file_at(0x1010), Some((0x1000, 0x20)));
        assert_eq!(fs.file_at(0x1020), None);
        assert_eq!(fs.file_at(0x200F), Some((0x2000, 0x10)));
    }

    #[test]
    fn shifts_offsets() {
        let fs = FileSystem::new(fst(), 2).unwrap();
        assert_eq!(fs.file_at(0x4000), Some((0x4000, 0x20)));
    }

    #[test]
    fn refuses_a_bad_table() {
        let mut bad = fst();
        bad[8..12].copy_from_slice(&9u32.to_be_bytes());
        assert!(FileSystem::new(bad, 0).is_none());
        let mut unterminated = fst();
        *unterminated.last_mut().unwrap() = b'x';
        assert!(FileSystem::new(unterminated, 0).is_none());
    }
}
