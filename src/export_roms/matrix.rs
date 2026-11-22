use super::super::test_support::TestEnv;

/// The dat nearly every export cell imports.
const DAT: &str = "Test System (20200721).dat";
/// The dat for the GameCube/Wii cells.
const RVZ_DAT: &str = "Test System (20230618) (RVZ).dat";
/// The dat for the single track CD cells.
const SINGLE_TRACK_DAT: &str = "Test System (20240229) (Single Track).dat";
/// The dat for the Wii U cells.
const WUD_DAT: &str = "Test System (20261025) (WUD).dat";

/// One cell of the export matrix: a world with its fixtures imported, a
/// `export-roms` invocation, and the files the destination directory holds
/// afterwards.
pub struct ExportCase<'a> {
    /// The dat that gives the world its one system.
    pub dat: &'a str,
    /// Fixtures copied into the temporary directory and imported, in order.
    pub roms: &'a [&'a str],
    /// The `--format` to export to.
    pub format: &'a str,
    /// The files the destination directory should hold, relative to it.
    pub files: &'a [&'a str],
}

const DEFAULT: ExportCase<'static> = ExportCase {
    dat: DAT,
    roms: &[],
    format: "",
    files: &[],
};

/// Default cell on the GameCube/Wii dat.
const RVZ: ExportCase<'static> = ExportCase {
    dat: RVZ_DAT,
    ..DEFAULT
};
/// Default cell on the single track dat.
const SINGLE_TRACK: ExportCase<'static> = ExportCase {
    dat: SINGLE_TRACK_DAT,
    ..DEFAULT
};
/// Default cell on the Wii U dat.
const WUD: ExportCase<'static> = ExportCase {
    dat: WUD_DAT,
    ..DEFAULT
};

macro_rules! export_cases {
    ($($name:ident : $case:expr),+ $(,)?) => {
        $(
            #[tokio::test]
            async fn $name() {
                run_export_case(&$case).await;
            }
        )+
    };
}

async fn run_export_case(case: &ExportCase<'_>) {
    let mut env = TestEnv::with_dat(case.dat).await;

    let mut paths = vec![];
    for rom in case.roms {
        paths.push(env.fixture(rom).await);
    }
    env.import_roms(&paths.iter().map(|path| path.as_path()).collect::<Vec<_>>())
        .await;

    let system = env.system().await;
    let destination = env.tmp_directory.join("destination");
    std::fs::create_dir_all(&destination).unwrap();

    env.run(&[
        "export-roms",
        "--system",
        &system.name,
        "--format",
        case.format,
        "--directory",
        destination.to_str().unwrap(),
    ])
    .await
    .unwrap();

    for file in case.files {
        assert!(
            destination.join(file).is_file(),
            "\"{file}\" should have been exported"
        );
    }
}

export_cases! {
    // PlayStation: ISO, CHD, CSO, ZSO, archives.
    cso_to_chd: ExportCase {
        roms: &["Test Game (USA, Europe).cso"],
        format: "CHD",
        files: &["Test Game (USA, Europe) (ISO).chd"],
        ..DEFAULT
    },
    cso_to_cso: ExportCase {
        roms: &["Test Game (USA, Europe).cso"],
        format: "CSO",
        files: &["Test Game (USA, Europe) (ISO).cso"],
        ..DEFAULT
    },
    cso_to_iso: ExportCase {
        roms: &["Test Game (USA, Europe).cso"],
        format: "ORIGINAL",
        files: &["Test Game (USA, Europe) (ISO).iso"],
        ..DEFAULT
    },
    cso_to_sevenzip_iso: ExportCase {
        roms: &["Test Game (USA, Europe).cso"],
        format: "7Z",
        files: &["Test Game (USA, Europe) (ISO).7z"],
        ..DEFAULT
    },
    iso_chd_to_chd_should_copy: ExportCase {
        roms: &["Test Game (USA, Europe) (ISO).chd"],
        format: "CHD",
        files: &["Test Game (USA, Europe) (ISO).chd"],
        ..DEFAULT
    },
    iso_chd_to_cso: ExportCase {
        roms: &["Test Game (USA, Europe) (ISO).chd"],
        format: "CSO",
        files: &["Test Game (USA, Europe) (ISO).cso"],
        ..DEFAULT
    },
    iso_chd_to_iso: ExportCase {
        roms: &["Test Game (USA, Europe) (ISO).chd"],
        format: "ORIGINAL",
        files: &["Test Game (USA, Europe) (ISO).iso"],
        ..DEFAULT
    },
    iso_chd_to_sevenzip_iso: ExportCase {
        roms: &["Test Game (USA, Europe) (ISO).chd"],
        format: "7Z",
        files: &["Test Game (USA, Europe) (ISO).7z"],
        ..DEFAULT
    },
    iso_chd_to_zso: ExportCase {
        roms: &["Test Game (USA, Europe) (ISO).chd"],
        format: "ZSO",
        files: &["Test Game (USA, Europe) (ISO).zso"],
        ..DEFAULT
    },
    iso_to_chd: ExportCase {
        roms: &["Test Game (USA, Europe).iso"],
        format: "CHD",
        files: &["Test Game (USA, Europe) (ISO).chd"],
        ..DEFAULT
    },
    iso_to_cso: ExportCase {
        roms: &["Test Game (USA, Europe).iso"],
        format: "CSO",
        files: &["Test Game (USA, Europe) (ISO).cso"],
        ..DEFAULT
    },
    iso_to_zso: ExportCase {
        roms: &["Test Game (USA, Europe).iso"],
        format: "ZSO",
        files: &["Test Game (USA, Europe) (ISO).zso"],
        ..DEFAULT
    },
    zso_to_chd: ExportCase {
        roms: &["Test Game (USA, Europe).zso"],
        format: "CHD",
        files: &["Test Game (USA, Europe) (ISO).chd"],
        ..DEFAULT
    },
    zso_to_iso: ExportCase {
        roms: &["Test Game (USA, Europe).zso"],
        format: "ORIGINAL",
        files: &["Test Game (USA, Europe) (ISO).iso"],
        ..DEFAULT
    },
    zso_to_sevenzip_iso: ExportCase {
        roms: &["Test Game (USA, Europe).zso"],
        format: "7Z",
        files: &["Test Game (USA, Europe) (ISO).7z"],
        ..DEFAULT
    },
    zso_to_zso_should_copy: ExportCase {
        roms: &["Test Game (USA, Europe).zso"],
        format: "ZSO",
        files: &["Test Game (USA, Europe) (ISO).zso"],
        ..DEFAULT
    },
    // Loose files and archives.
    original_to_original_should_copy: ExportCase {
        roms: &["Test Game (USA, Europe).rom"],
        format: "ORIGINAL",
        files: &["Test Game (USA, Europe).rom"],
        ..DEFAULT
    },
    original_to_sevenzip: ExportCase {
        roms: &["Test Game (USA, Europe).rom"],
        format: "7Z",
        files: &["Test Game (USA, Europe).7z"],
        ..DEFAULT
    },
    original_to_zip: ExportCase {
        roms: &["Test Game (USA, Europe).rom"],
        format: "ZIP",
        files: &["Test Game (USA, Europe).zip"],
        ..DEFAULT
    },
    sevenzip_to_original: ExportCase {
        roms: &["Test Game (USA, Europe).rom.7z"],
        format: "ORIGINAL",
        files: &["Test Game (USA, Europe).rom"],
        ..DEFAULT
    },
    sevenzip_to_zip: ExportCase {
        roms: &["Test Game (USA, Europe).rom.7z"],
        format: "ZIP",
        files: &["Test Game (USA, Europe).zip"],
        ..DEFAULT
    },
    zip_to_original: ExportCase {
        roms: &["Test Game (USA, Europe).rom.zip"],
        format: "ORIGINAL",
        files: &["Test Game (USA, Europe).rom"],
        ..DEFAULT
    },
    zip_to_sevenzip: ExportCase {
        roms: &["Test Game (USA, Europe).rom.zip"],
        format: "7Z",
        files: &["Test Game (USA, Europe).7z"],
        ..DEFAULT
    },
    zip_to_zip_should_copy: ExportCase {
        roms: &["Test Game (USA, Europe).rom.zip"],
        format: "ZIP",
        files: &["Test Game (USA, Europe).zip"],
        ..DEFAULT
    },
    // Archived discs.
    sevenzip_iso_to_chd: ExportCase {
        roms: &["Test Game (USA, Europe).iso.7z"],
        format: "CHD",
        files: &["Test Game (USA, Europe) (ISO).chd"],
        ..DEFAULT
    },
    sevenzip_iso_to_cso: ExportCase {
        roms: &["Test Game (USA, Europe).iso.7z"],
        format: "CSO",
        files: &["Test Game (USA, Europe) (ISO).cso"],
        ..DEFAULT
    },
    sevenzip_iso_to_zso: ExportCase {
        roms: &["Test Game (USA, Europe).iso.7z"],
        format: "ZSO",
        files: &["Test Game (USA, Europe) (ISO).zso"],
        ..DEFAULT
    },
    // Compact discs: whole-archive exports of the multi track set.
    multiple_tracks_chd_to_cue_bin: ExportCase {
        roms: &[
            "Test Game (USA, Europe) (Multiple Tracks).cue",
            "Test Game (USA, Europe) (Multiple Tracks).chd",
        ],
        format: "ORIGINAL",
        files: &[
            "Test Game (USA, Europe) (CUE BIN) (Track 01).bin",
            "Test Game (USA, Europe) (CUE BIN) (Track 02).bin",
            "Test Game (USA, Europe) (CUE BIN).cue",
        ],
        ..DEFAULT
    },
    multiple_tracks_chd_to_sevenzip_cue_bin: ExportCase {
        roms: &[
            "Test Game (USA, Europe) (Multiple Tracks).cue",
            "Test Game (USA, Europe) (Multiple Tracks).chd",
        ],
        format: "7Z",
        files: &["Test Game (USA, Europe) (CUE BIN).7z"],
        ..DEFAULT
    },
    multiple_tracks_cue_bin_to_chd: ExportCase {
        roms: &[
            "Test Game (USA, Europe) (Multiple Tracks).cue",
            "Test Game (USA, Europe) (CUE BIN) (Track 01).bin",
            "Test Game (USA, Europe) (CUE BIN) (Track 02).bin",
        ],
        format: "CHD",
        files: &["Test Game (USA, Europe) (CUE BIN).chd"],
        ..DEFAULT
    },
    // Single track CDs: the archived and loose sets export to one ISO.
    single_track_chd_to_iso: ExportCase {
        roms: &[
            "Test Game (USA, Europe) (Single Track).cue",
            "Test Game (USA, Europe) (Single Track).chd",
        ],
        format: "ISO",
        files: &["Test Game (USA, Europe) (CUE BIN).iso"],
        ..SINGLE_TRACK
    },
    sevenzip_single_track_cue_bin_to_iso: ExportCase {
        roms: &["Test Game (USA, Europe) (Single Track).7z"],
        format: "ISO",
        files: &["Test Game (USA, Europe) (CUE BIN).iso"],
        ..SINGLE_TRACK
    },
    // GameCube and Wii.
    iso_to_rvz: ExportCase {
        roms: &["Test Game (USA).iso"],
        format: "RVZ",
        files: &["Test Game (USA).rvz"],
        ..RVZ
    },
    iso_to_wbfs: ExportCase {
        roms: &["Test Game (USA).iso"],
        format: "WBFS",
        files: &["Test Game (USA).wbfs"],
        ..RVZ
    },
    rvz_to_iso: ExportCase {
        roms: &["Test Game (USA).rvz"],
        format: "ORIGINAL",
        files: &["Test Game (USA).iso"],
        ..RVZ
    },
    rvz_to_rvz_should_copy: ExportCase {
        roms: &["Test Game (USA).rvz"],
        format: "RVZ",
        files: &["Test Game (USA).rvz"],
        ..RVZ
    },
    rvz_to_sevenzip_iso: ExportCase {
        roms: &["Test Game (USA).rvz"],
        format: "7Z",
        files: &["Test Game (USA).7z"],
        ..RVZ
    },
    rvz_to_wbfs: ExportCase {
        roms: &["Test Game (USA).rvz"],
        format: "WBFS",
        files: &["Test Game (USA).wbfs"],
        ..RVZ
    },
    // Wii U.
    wud_to_wua: ExportCase {
        roms: &["Test Game (USA).wud"],
        format: "WUA",
        files: &["Test Game (USA).wua"],
        ..WUD
    },
    wux_to_wud: ExportCase {
        roms: &["Test Game (USA).wux"],
        format: "ORIGINAL",
        files: &["Test Game (USA).iso"],
        ..WUD
    },
}
