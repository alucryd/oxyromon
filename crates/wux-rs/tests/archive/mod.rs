//! Reading a WUA with an independent reader, zarust, wux-rs's alone.

/// Every file in the ZArchive at `path`, with its contents, read with an
/// independent reader, sorted by path.
pub fn archive_files(path: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    use zarust::{ArchiveReader, ROOT_NODE};
    let mut reader = ArchiveReader::open(path).unwrap();
    assert!(reader.verify_integrity().unwrap());
    let mut files = Vec::new();
    let mut dirs = vec![(String::new(), ROOT_NODE)];
    while let Some((prefix, handle)) = dirs.pop() {
        let entries: Vec<_> = reader
            .directory_entries(handle)
            .unwrap()
            .iter()
            .map(|e| {
                (
                    String::from_utf8(e.name.to_vec()).unwrap(),
                    e.is_file(),
                    e.handle,
                )
            })
            .collect();
        for (name, is_file, handle) in entries {
            let path = format!("{prefix}{name}");
            if is_file {
                files.push((path, reader.read_file_to_end(handle).unwrap()));
            } else {
                dirs.push((format!("{path}/"), handle));
            }
        }
    }
    files.sort();
    files
}
