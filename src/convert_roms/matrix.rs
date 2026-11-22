use super::super::database::find_roms_with_romfile_by_system_id;
use super::super::test_support::TestEnv;
use std::path::Path;
use std::time::SystemTime;

/// The dat nearly every convert cell imports.
const DAT: &str = "Test System (20200721).dat";
/// The dat for the GameCube/Wii cells.
const RVZ_DAT: &str = "Test System (20230618) (RVZ).dat";
/// The dat for the Wii U cells.
const WUD_DAT: &str = "Test System (20261025) (WUD).dat";

/// One cell of the convert matrix: a world with its fixtures imported,
/// one or more `convert-roms` invocations, and what the database and the
/// disk hold afterwards. Leave a field at `DEFAULT` to take the common
/// value.
pub struct ConvertCase<'a> {
    /// The dat that gives the world its one system.
    pub dat: &'a str,
    /// Fixtures copied into the temporary directory and imported, in order.
    pub roms: &'a [&'a str],
    /// `convert-roms` invocations, each its own step, `--all` included.
    pub steps: &'a [&'a [&'a str]],
    /// The rom names the database should hold afterwards, sorted. Empty
    /// asserts the single rom a one-file conversion leaves.
    pub rom_names: &'a [&'a str],
    /// The romfile paths, relative to the system directory, sorted.
    pub romfiles: &'a [&'a str],
    /// Whether the conversion must leave a fresh file: a conversion to a
    /// format the rom is already in, run with `--recompress`.
    pub recreated: bool,
}

const DEFAULT: ConvertCase<'static> = ConvertCase {
    dat: DAT,
    roms: &[],
    steps: &[],
    rom_names: &[],
    romfiles: &[],
    recreated: false,
};

macro_rules! convert_cases {
    ($($name:ident : $case:expr),+ $(,)?) => {
        $(
            #[tokio::test]
            async fn $name() {
                run_convert_case(&$case).await;
            }
        )+
    };
}

async fn run_convert_case(case: &ConvertCase<'_>) {
    let mut env = TestEnv::with_dat(case.dat).await;

    let mut paths = vec![];
    for rom in case.roms {
        paths.push(env.fixture(rom).await);
    }
    env.import_roms(&paths.iter().map(|path| path.as_path()).collect::<Vec<_>>())
        .await;

    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

    let before = if case.recreated {
        let romfile = env.single_romfile().await;
        Some(file_time(&env.romfile_path(&romfile)))
    } else {
        None
    };

    for step in case.steps {
        let mut argv = vec!["convert-roms", "--all"];
        argv.extend_from_slice(step);
        env.run(&argv).await.unwrap();
    }

    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    if case.rom_names.is_empty() {
        assert_eq!(roms.len(), 1, "the conversion should have left one rom");
    } else {
        let mut names: Vec<&str> = roms.iter().map(|rom| rom.name.as_str()).collect();
        names.sort_unstable();
        let mut expected = case.rom_names.to_vec();
        expected.sort_unstable();
        assert_eq!(names, expected, "the roms left in the database");
    }

    let mut romfiles = env.romfiles().await;
    let mut expected: Vec<String> = case
        .romfiles
        .iter()
        .map(|name| env.stored_path(&system_directory.join(name)))
        .collect();
    expected.sort();
    let mut actual: Vec<String> = romfiles
        .iter()
        .map(|romfile| romfile.path.clone())
        .collect();
    actual.sort();
    assert_eq!(actual, expected, "the romfiles left in the database");

    for romfile in romfiles.iter() {
        assert!(
            env.rom_directory.join(&romfile.path).is_file(),
            "\"{}\" should be on disk",
            romfile.path
        );
    }
    if roms.len() == 1 && romfiles.len() == 1 {
        assert_eq!(roms[0].romfile_id, Some(romfiles[0].id));
    }

    if let Some(before) = before {
        let romfile = env.single_romfile().await;
        assert_ne!(
            before,
            file_time(&env.romfile_path(&romfile)),
            "the conversion should have rewritten the file"
        );
    }
}

fn file_time(path: &Path) -> SystemTime {
    std::fs::metadata(path).unwrap().modified().unwrap()
}

convert_cases! {
    // PlayStation: ISO, CHD, CSO, ZSO, archives.
    cso_to_chd: ConvertCase {
        roms: &["Test Game (USA, Europe).cso"],
        steps: &[&["--format", "CHD"]],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).chd"],
        ..DEFAULT
    },
    cso_to_cso: ConvertCase {
        roms: &["Test Game (USA, Europe).cso"],
        steps: &[&["--format", "CSO", "--recompress"]],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).cso"],
        recreated: true,
        ..DEFAULT
    },
    cso_to_iso: ConvertCase {
        roms: &["Test Game (USA, Europe).cso"],
        steps: &[&["--format", "ORIGINAL"]],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).iso"],
        ..DEFAULT
    },
    cso_to_sevenzip_iso: ConvertCase {
        roms: &["Test Game (USA, Europe).cso"],
        steps: &[&["--format", "7Z"]],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).7z"],
        ..DEFAULT
    },
    cso_to_zso: ConvertCase {
        roms: &["Test Game (USA, Europe).cso"],
        steps: &[&["--format", "ZSO"]],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).zso"],
        ..DEFAULT
    },
    iso_chd_to_chd: ConvertCase {
        roms: &["Test Game (USA, Europe) (ISO).chd"],
        steps: &[&["--format", "CHD", "--recompress"]],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).chd"],
        recreated: true,
        ..DEFAULT
    },
    iso_chd_to_cso: ConvertCase {
        roms: &["Test Game (USA, Europe) (ISO).chd"],
        steps: &[&["--format", "CSO"]],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).cso"],
        ..DEFAULT
    },
    iso_chd_to_iso: ConvertCase {
        roms: &["Test Game (USA, Europe) (ISO).chd"],
        steps: &[&["--format", "ORIGINAL"]],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).iso"],
        ..DEFAULT
    },
    iso_chd_to_sevenzip_iso: ConvertCase {
        roms: &["Test Game (USA, Europe) (ISO).chd"],
        steps: &[&["--format", "7Z"]],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).7z"],
        ..DEFAULT
    },
    iso_chd_to_zso: ConvertCase {
        roms: &["Test Game (USA, Europe) (ISO).chd"],
        steps: &[&["--format", "ZSO"]],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).zso"],
        ..DEFAULT
    },
    iso_to_chd: ConvertCase {
        roms: &["Test Game (USA, Europe).iso"],
        steps: &[&["--format", "CHD"]],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).chd"],
        ..DEFAULT
    },
    iso_to_cso: ConvertCase {
        roms: &["Test Game (USA, Europe).iso"],
        steps: &[&["--format", "CSO"]],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).cso"],
        ..DEFAULT
    },
    iso_to_zso: ConvertCase {
        roms: &["Test Game (USA, Europe).iso"],
        steps: &[&["--format", "ZSO"]],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).zso"],
        ..DEFAULT
    },
    zso_to_chd: ConvertCase {
        roms: &["Test Game (USA, Europe).zso"],
        steps: &[&["--format", "CHD"]],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).chd"],
        ..DEFAULT
    },
    zso_to_cso: ConvertCase {
        roms: &["Test Game (USA, Europe).zso"],
        steps: &[&["--format", "CSO"]],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).cso"],
        ..DEFAULT
    },
    zso_to_iso: ConvertCase {
        roms: &["Test Game (USA, Europe).zso"],
        steps: &[&["--format", "ORIGINAL"]],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).iso"],
        ..DEFAULT
    },
    zso_to_sevenzip_iso: ConvertCase {
        roms: &["Test Game (USA, Europe).zso"],
        steps: &[&["--format", "7Z"]],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).7z"],
        ..DEFAULT
    },
    zso_to_zso: ConvertCase {
        roms: &["Test Game (USA, Europe).zso"],
        steps: &[&["--format", "ZSO", "--recompress"]],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).zso"],
        recreated: true,
        ..DEFAULT
    },
    // Loose files and archives.
    original_to_sevenzip: ConvertCase {
        roms: &["Test Game (USA, Europe).rom"],
        steps: &[&["--format", "7Z"]],
        rom_names: &["Test Game (USA, Europe).rom"],
        romfiles: &["Test Game (USA, Europe).7z"],
        ..DEFAULT
    },
    original_to_zip: ConvertCase {
        roms: &["Test Game (USA, Europe).rom"],
        steps: &[&["--format", "ZIP"]],
        rom_names: &["Test Game (USA, Europe).rom"],
        romfiles: &["Test Game (USA, Europe).zip"],
        ..DEFAULT
    },
    sevenzip_to_original: ConvertCase {
        roms: &["Test Game (USA, Europe).rom.7z"],
        steps: &[&["--format", "ORIGINAL"]],
        rom_names: &["Test Game (USA, Europe).rom"],
        romfiles: &["Test Game (USA, Europe).rom"],
        ..DEFAULT
    },
    sevenzip_to_sevenzip: ConvertCase {
        roms: &["Test Game (USA, Europe).rom.7z"],
        steps: &[&["--format", "7Z", "--recompress"]],
        rom_names: &["Test Game (USA, Europe).rom"],
        romfiles: &["Test Game (USA, Europe).7z"],
        recreated: true,
        ..DEFAULT
    },
    sevenzip_to_zip: ConvertCase {
        roms: &["Test Game (USA, Europe).rom.7z"],
        steps: &[&["--format", "ZIP"]],
        rom_names: &["Test Game (USA, Europe).rom"],
        romfiles: &["Test Game (USA, Europe).zip"],
        ..DEFAULT
    },
    zip_to_original: ConvertCase {
        roms: &["Test Game (USA, Europe).rom.zip"],
        steps: &[&["--format", "ORIGINAL"]],
        rom_names: &["Test Game (USA, Europe).rom"],
        romfiles: &["Test Game (USA, Europe).rom"],
        ..DEFAULT
    },
    zip_to_sevenzip: ConvertCase {
        roms: &["Test Game (USA, Europe).rom.zip"],
        steps: &[&["--format", "7Z"]],
        rom_names: &["Test Game (USA, Europe).rom"],
        romfiles: &["Test Game (USA, Europe).7z"],
        ..DEFAULT
    },
    zip_to_zip: ConvertCase {
        roms: &["Test Game (USA, Europe).rom.zip"],
        steps: &[&["--format", "ZIP", "--recompress"]],
        rom_names: &["Test Game (USA, Europe).rom"],
        romfiles: &["Test Game (USA, Europe).zip"],
        recreated: true,
        ..DEFAULT
    },
    // Archived discs.
    sevenzip_iso_to_chd: ConvertCase {
        roms: &["Test Game (USA, Europe).iso.7z"],
        steps: &[&["--format", "CHD"]],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).chd"],
        ..DEFAULT
    },
    sevenzip_iso_to_cso: ConvertCase {
        roms: &["Test Game (USA, Europe).iso.7z"],
        steps: &[&["--format", "CSO"]],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).cso"],
        ..DEFAULT
    },
    sevenzip_iso_to_zso: ConvertCase {
        roms: &["Test Game (USA, Europe).iso.7z"],
        steps: &[&["--format", "ZSO"]],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).zso"],
        ..DEFAULT
    },
    // GameCube and Wii.
    iso_to_ore: ConvertCase {
        dat: RVZ_DAT,
        roms: &["Test Game (USA).iso"],
        steps: &[&["--format", "ORE"]],
        rom_names: &["Test Game (USA).iso"],
        romfiles: &["Test Game (USA).ore"],
        ..DEFAULT
    },
    iso_to_rvz: ConvertCase {
        dat: RVZ_DAT,
        roms: &["Test Game (USA).iso"],
        steps: &[&["--format", "RVZ"]],
        rom_names: &["Test Game (USA).iso"],
        romfiles: &["Test Game (USA).rvz"],
        ..DEFAULT
    },
    rvz_to_iso: ConvertCase {
        dat: RVZ_DAT,
        roms: &["Test Game (USA).rvz"],
        steps: &[&["--format", "ORIGINAL"]],
        rom_names: &["Test Game (USA).iso"],
        romfiles: &["Test Game (USA).iso"],
        ..DEFAULT
    },
    rvz_to_ore: ConvertCase {
        dat: RVZ_DAT,
        roms: &["Test Game (USA).rvz"],
        steps: &[&["--format", "ORE"]],
        rom_names: &["Test Game (USA).iso"],
        romfiles: &["Test Game (USA).ore"],
        ..DEFAULT
    },
    rvz_to_rvz: ConvertCase {
        dat: RVZ_DAT,
        roms: &["Test Game (USA).rvz"],
        steps: &[&["--format", "RVZ", "--recompress"]],
        rom_names: &["Test Game (USA).iso"],
        romfiles: &["Test Game (USA).rvz"],
        recreated: true,
        ..DEFAULT
    },
    rvz_to_sevenzip_iso: ConvertCase {
        dat: RVZ_DAT,
        roms: &["Test Game (USA).rvz"],
        steps: &[&["--format", "7Z"]],
        rom_names: &["Test Game (USA).iso"],
        romfiles: &["Test Game (USA).7z"],
        ..DEFAULT
    },
    ore_to_iso: ConvertCase {
        dat: RVZ_DAT,
        roms: &["Test Game (USA).iso"],
        steps: &[&["--format", "ORIGINAL"]],
        rom_names: &["Test Game (USA).iso"],
        romfiles: &["Test Game (USA).iso"],
        ..DEFAULT
    },
    // Wii U.
    wud_to_wux: ConvertCase {
        dat: WUD_DAT,
        roms: &["Test Game (USA).wud"],
        steps: &[&["--format", "WUX"]],
        rom_names: &["Test Game (USA).iso"],
        romfiles: &["Test Game (USA).wux"],
        ..DEFAULT
    },
    wux_to_sevenzip_wud: ConvertCase {
        dat: WUD_DAT,
        roms: &["Test Game (USA).wux"],
        steps: &[&["--format", "7Z"]],
        rom_names: &["Test Game (USA).iso"],
        romfiles: &["Test Game (USA).7z"],
        ..DEFAULT
    },
    wux_to_wud: ConvertCase {
        dat: WUD_DAT,
        roms: &["Test Game (USA).wux"],
        steps: &[&["--format", "ORIGINAL"]],
        rom_names: &["Test Game (USA).iso"],
        romfiles: &["Test Game (USA).iso"],
        ..DEFAULT
    },
}
