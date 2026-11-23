use super::*;

#[test]
fn test() {
    // given
    let cue: &[u8] = b"REM COMMENT \"\xe9\"\r\n\
        FILE \"Old (Track 1).bin\" BINARY\r\n\
        \x20 TRACK 01 MODE1/2352\r\n\
        \x20   INDEX 01 00:00:00\r\n\
        \x20 file \"Old (Track 2).bin\" BINARY\r\n\
        \x20 TRACK 02 AUDIO\r\n\
        FILE unquoted.bin BINARY\r\n\
        FILEX \"Old (Track 1).bin\"\r\n\
        FILE \"Kept.bin\" BINARY";
    let new_names = HashMap::from([
        ("Old (Track 1).bin", String::from("New (Track 1).bin")),
        ("Old (Track 2).bin", String::from("New (Track 2).bin")),
    ]);

    // when
    let names = get_cue_file_names(cue);
    let renamed = rename_cue_files(cue, &new_names);

    // then
    assert_eq!(
        names,
        ["Old (Track 1).bin", "Old (Track 2).bin", "Kept.bin"]
    );
    assert_eq!(
        renamed,
        b"REM COMMENT \"\xe9\"\r\n\
        FILE \"New (Track 1).bin\" BINARY\r\n\
        \x20 TRACK 01 MODE1/2352\r\n\
        \x20   INDEX 01 00:00:00\r\n\
        \x20 file \"New (Track 2).bin\" BINARY\r\n\
        \x20 TRACK 02 AUDIO\r\n\
        FILE unquoted.bin BINARY\r\n\
        FILEX \"Old (Track 1).bin\"\r\n\
        FILE \"Kept.bin\" BINARY"
    );
}
