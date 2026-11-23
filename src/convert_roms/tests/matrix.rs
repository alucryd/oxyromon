use crate::test_support::{DAT, RVZ_DAT, TestEnv, WUD_DAT, test_cases};
use std::path::Path;
use std::time::SystemTime;

/// One cell of the convert matrix: a world with its fixtures imported,
/// a `convert-roms` invocation, and what the database and the
/// disk hold afterwards. Leave a field at `DEFAULT` to take the common
/// value.
struct ConvertCase<'a> {
    /// The dat that gives the world its one system.
    dat: &'a str,
    /// Fixtures copied into the temporary directory and imported, in order.
    roms: &'a [&'a str],
    /// `convert-roms` arguments beyond `--all`.
    args: &'a [&'a str],
    /// The rom names the database should hold afterwards, sorted. Empty
    /// asserts the single rom a one-file conversion leaves.
    rom_names: &'a [&'a str],
    /// The romfile paths, relative to the system directory, sorted.
    romfiles: &'a [&'a str],
    /// Whether the conversion must leave a fresh file: a conversion to a
    /// format the rom is already in, run with `--recompress`.
    recreated: bool,
    /// Compress each fixture to ORE with ore-rs and import that instead:
    /// an input the repo doesn't check in.
    ore: bool,
}

const DEFAULT: ConvertCase<'static> = ConvertCase {
    dat: DAT,
    roms: &[],
    args: &[],
    rom_names: &[],
    romfiles: &[],
    recreated: false,
    ore: false,
};

async fn run_convert_case(case: &ConvertCase<'_>) {
    let mut env = TestEnv::with_dat(case.dat).await;

    let mut paths = vec![];
    for rom in case.roms {
        if case.ore {
            let path = env.tmp_directory.join(rom).with_extension("ore");
            ore_rs::compress(
                &env.fixture_source(rom),
                &path,
                &ore_rs::Options::default(),
                &[],
                &mut |_| {},
            )
            .unwrap();
            paths.push(path);
        } else {
            paths.push(env.fixture(rom).await);
        }
    }
    env.import_roms(&paths.iter().map(|path| path.as_path()).collect::<Vec<_>>())
        .await;

    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

    if case.ore {
        // Otherwise the cell converts its input to the format it is already in.
        assert!(env.single_romfile().await.path.ends_with(".ore"));
    }

    let before = if case.recreated {
        let romfile = env.single_romfile().await;
        Some(file_time(&env.romfile_path(&romfile)))
    } else {
        None
    };

    let mut argv = vec!["convert-roms", "--all"];
    argv.extend_from_slice(case.args);
    env.run(&argv).await.unwrap();

    let roms = env.roms_with_romfiles(&system).await;
    if case.rom_names.is_empty() {
        assert_eq!(roms.len(), 1, "the conversion should have left one rom");
    } else {
        let mut names: Vec<&str> = roms.iter().map(|rom| rom.name.as_str()).collect();
        names.sort_unstable();
        let mut expected = case.rom_names.to_vec();
        expected.sort_unstable();
        assert_eq!(names, expected, "the roms left in the database");
    }

    let romfiles = env.romfiles().await;
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

test_cases! {
    run_convert_case;
    // PlayStation: ISO, CHD, CSO, ZSO, archives.
    cso_to_chd: ConvertCase {
        roms: &["Test Game (USA, Europe).cso"],
        args: &["--format", "CHD"],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).chd"],
        ..DEFAULT
    },
    cso_to_cso: ConvertCase {
        roms: &["Test Game (USA, Europe).cso"],
        args: &["--format", "CSO", "--recompress"],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).cso"],
        recreated: true,
        ..DEFAULT
    },
    cso_to_iso: ConvertCase {
        roms: &["Test Game (USA, Europe).cso"],
        args: &["--format", "ORIGINAL"],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).iso"],
        ..DEFAULT
    },
    cso_to_sevenzip_iso: ConvertCase {
        roms: &["Test Game (USA, Europe).cso"],
        args: &["--format", "7Z"],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).7z"],
        ..DEFAULT
    },
    cso_to_zso: ConvertCase {
        roms: &["Test Game (USA, Europe).cso"],
        args: &["--format", "ZSO"],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).zso"],
        ..DEFAULT
    },
    iso_chd_to_chd: ConvertCase {
        roms: &["Test Game (USA, Europe) (ISO).chd"],
        args: &["--format", "CHD", "--recompress"],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).chd"],
        recreated: true,
        ..DEFAULT
    },
    iso_chd_to_cso: ConvertCase {
        roms: &["Test Game (USA, Europe) (ISO).chd"],
        args: &["--format", "CSO"],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).cso"],
        ..DEFAULT
    },
    iso_chd_to_iso: ConvertCase {
        roms: &["Test Game (USA, Europe) (ISO).chd"],
        args: &["--format", "ORIGINAL"],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).iso"],
        ..DEFAULT
    },
    iso_chd_to_sevenzip_iso: ConvertCase {
        roms: &["Test Game (USA, Europe) (ISO).chd"],
        args: &["--format", "7Z"],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).7z"],
        ..DEFAULT
    },
    iso_chd_to_zso: ConvertCase {
        roms: &["Test Game (USA, Europe) (ISO).chd"],
        args: &["--format", "ZSO"],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).zso"],
        ..DEFAULT
    },
    iso_to_chd: ConvertCase {
        roms: &["Test Game (USA, Europe).iso"],
        args: &["--format", "CHD"],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).chd"],
        ..DEFAULT
    },
    iso_to_cso: ConvertCase {
        roms: &["Test Game (USA, Europe).iso"],
        args: &["--format", "CSO"],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).cso"],
        ..DEFAULT
    },
    iso_to_zso: ConvertCase {
        roms: &["Test Game (USA, Europe).iso"],
        args: &["--format", "ZSO"],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).zso"],
        ..DEFAULT
    },
    zso_to_chd: ConvertCase {
        roms: &["Test Game (USA, Europe).zso"],
        args: &["--format", "CHD"],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).chd"],
        ..DEFAULT
    },
    zso_to_cso: ConvertCase {
        roms: &["Test Game (USA, Europe).zso"],
        args: &["--format", "CSO"],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).cso"],
        ..DEFAULT
    },
    zso_to_iso: ConvertCase {
        roms: &["Test Game (USA, Europe).zso"],
        args: &["--format", "ORIGINAL"],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).iso"],
        ..DEFAULT
    },
    zso_to_sevenzip_iso: ConvertCase {
        roms: &["Test Game (USA, Europe).zso"],
        args: &["--format", "7Z"],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).7z"],
        ..DEFAULT
    },
    zso_to_zso: ConvertCase {
        roms: &["Test Game (USA, Europe).zso"],
        args: &["--format", "ZSO", "--recompress"],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).zso"],
        recreated: true,
        ..DEFAULT
    },
    // Loose files and archives.
    original_to_sevenzip: ConvertCase {
        roms: &["Test Game (USA, Europe).rom"],
        args: &["--format", "7Z"],
        rom_names: &["Test Game (USA, Europe).rom"],
        romfiles: &["Test Game (USA, Europe).7z"],
        ..DEFAULT
    },
    original_to_zip: ConvertCase {
        roms: &["Test Game (USA, Europe).rom"],
        args: &["--format", "ZIP"],
        rom_names: &["Test Game (USA, Europe).rom"],
        romfiles: &["Test Game (USA, Europe).zip"],
        ..DEFAULT
    },
    sevenzip_to_original: ConvertCase {
        roms: &["Test Game (USA, Europe).rom.7z"],
        args: &["--format", "ORIGINAL"],
        rom_names: &["Test Game (USA, Europe).rom"],
        romfiles: &["Test Game (USA, Europe).rom"],
        ..DEFAULT
    },
    sevenzip_to_sevenzip: ConvertCase {
        roms: &["Test Game (USA, Europe).rom.7z"],
        args: &["--format", "7Z", "--recompress"],
        rom_names: &["Test Game (USA, Europe).rom"],
        romfiles: &["Test Game (USA, Europe).7z"],
        recreated: true,
        ..DEFAULT
    },
    sevenzip_to_zip: ConvertCase {
        roms: &["Test Game (USA, Europe).rom.7z"],
        args: &["--format", "ZIP"],
        rom_names: &["Test Game (USA, Europe).rom"],
        romfiles: &["Test Game (USA, Europe).zip"],
        ..DEFAULT
    },
    zip_to_original: ConvertCase {
        roms: &["Test Game (USA, Europe).rom.zip"],
        args: &["--format", "ORIGINAL"],
        rom_names: &["Test Game (USA, Europe).rom"],
        romfiles: &["Test Game (USA, Europe).rom"],
        ..DEFAULT
    },
    zip_to_sevenzip: ConvertCase {
        roms: &["Test Game (USA, Europe).rom.zip"],
        args: &["--format", "7Z"],
        rom_names: &["Test Game (USA, Europe).rom"],
        romfiles: &["Test Game (USA, Europe).7z"],
        ..DEFAULT
    },
    zip_to_zip: ConvertCase {
        roms: &["Test Game (USA, Europe).rom.zip"],
        args: &["--format", "ZIP", "--recompress"],
        rom_names: &["Test Game (USA, Europe).rom"],
        romfiles: &["Test Game (USA, Europe).zip"],
        recreated: true,
        ..DEFAULT
    },
    // Archived discs.
    sevenzip_iso_to_chd: ConvertCase {
        roms: &["Test Game (USA, Europe).iso.7z"],
        args: &["--format", "CHD"],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).chd"],
        ..DEFAULT
    },
    sevenzip_iso_to_cso: ConvertCase {
        roms: &["Test Game (USA, Europe).iso.7z"],
        args: &["--format", "CSO"],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).cso"],
        ..DEFAULT
    },
    sevenzip_iso_to_zso: ConvertCase {
        roms: &["Test Game (USA, Europe).iso.7z"],
        args: &["--format", "ZSO"],
        rom_names: &["Test Game (USA, Europe) (ISO).iso"],
        romfiles: &["Test Game (USA, Europe) (ISO).zso"],
        ..DEFAULT
    },
    // GameCube and Wii.
    iso_to_ore: ConvertCase {
        dat: RVZ_DAT,
        roms: &["Test Game (USA).iso"],
        args: &["--format", "ORE"],
        rom_names: &["Test Game (USA).iso"],
        romfiles: &["Test Game (USA).ore"],
        ..DEFAULT
    },
    iso_to_rvz: ConvertCase {
        dat: RVZ_DAT,
        roms: &["Test Game (USA).iso"],
        args: &["--format", "RVZ"],
        rom_names: &["Test Game (USA).iso"],
        romfiles: &["Test Game (USA).rvz"],
        ..DEFAULT
    },
    rvz_to_iso: ConvertCase {
        dat: RVZ_DAT,
        roms: &["Test Game (USA).rvz"],
        args: &["--format", "ORIGINAL"],
        rom_names: &["Test Game (USA).iso"],
        romfiles: &["Test Game (USA).iso"],
        ..DEFAULT
    },
    rvz_to_ore: ConvertCase {
        dat: RVZ_DAT,
        roms: &["Test Game (USA).rvz"],
        args: &["--format", "ORE"],
        rom_names: &["Test Game (USA).iso"],
        romfiles: &["Test Game (USA).ore"],
        ..DEFAULT
    },
    rvz_to_rvz: ConvertCase {
        dat: RVZ_DAT,
        roms: &["Test Game (USA).rvz"],
        args: &["--format", "RVZ", "--recompress"],
        rom_names: &["Test Game (USA).iso"],
        romfiles: &["Test Game (USA).rvz"],
        recreated: true,
        ..DEFAULT
    },
    rvz_to_sevenzip_iso: ConvertCase {
        dat: RVZ_DAT,
        roms: &["Test Game (USA).rvz"],
        args: &["--format", "7Z"],
        rom_names: &["Test Game (USA).iso"],
        romfiles: &["Test Game (USA).7z"],
        ..DEFAULT
    },
    ore_to_iso: ConvertCase {
        dat: RVZ_DAT,
        roms: &["Test Game (USA).iso"],
        args: &["--format", "ORIGINAL"],
        rom_names: &["Test Game (USA).iso"],
        romfiles: &["Test Game (USA).iso"],
        ore: true,
        ..DEFAULT
    },
    // Wii U.
    wud_to_wux: ConvertCase {
        dat: WUD_DAT,
        roms: &["Test Game (USA).wud"],
        args: &["--format", "WUX"],
        rom_names: &["Test Game (USA).iso"],
        romfiles: &["Test Game (USA).wux"],
        ..DEFAULT
    },
    wux_to_sevenzip_wud: ConvertCase {
        dat: WUD_DAT,
        roms: &["Test Game (USA).wux"],
        args: &["--format", "7Z"],
        rom_names: &["Test Game (USA).iso"],
        romfiles: &["Test Game (USA).7z"],
        ..DEFAULT
    },
    wux_to_wud: ConvertCase {
        dat: WUD_DAT,
        roms: &["Test Game (USA).wux"],
        args: &["--format", "ORIGINAL"],
        rom_names: &["Test Game (USA).iso"],
        romfiles: &["Test Game (USA).iso"],
        ..DEFAULT
    },
}
