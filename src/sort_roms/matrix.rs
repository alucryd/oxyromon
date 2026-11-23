use super::super::test_support::{DAT, TestEnv, test_cases};
use std::path::Path;

/// The dat with clone relations for 1G1R.
const PARENT_CLONE_DAT: &str = "Test System (20200721) (Parent-Clone).dat";
/// The dat with categories on the releases.
const CATEGORIES_DAT: &str = "Test System (20261109) (Parent-Clone) (Categories).dat";
/// The dat with revisions alongside the parent.
const REVISIONS_DAT: &str = "Test System (20230112) (Parent-Clone) (Revisions).dat";
/// The dat for the language-first cells.
const LANGUAGE_DAT: &str = "Language Test System (20200721).dat";

/// One value a sort cell writes into the system's settings before running.
enum Setting<'a> {
    Bool(&'a str, bool),
    Str(&'a str, &'a str),
    List(&'a str, &'a [&'a str]),
}

/// One cell of the sort matrix: a world with its fixtures imported, the
/// settings `sort-roms` reads back, the invocation, and where every
/// romfile ends up.
struct SortCase<'a> {
    /// The dat that gives the world its one system.
    dat: &'a str,
    /// Fixture roms, copied and imported one by one, in order.
    roms: &'a [&'a str],
    /// Settings applied to the system before sorting.
    settings: &'a [Setting<'a>],
    /// `sort-roms` arguments beyond `--all --yes`.
    args: &'a [&'a str],
    /// Where each romfile ends up: its name, and the folders between the
    /// system directory and it. Empty folders mean the system root.
    expected: &'a [(&'a str, &'a [&'a str])],
}

const DEFAULT: SortCase<'static> = SortCase {
    dat: DAT,
    roms: &[],
    settings: &[],
    args: &[],
    expected: &[],
};

/// Default cell on the parent-clone dat.
const PARENT_CLONE: SortCase<'static> = SortCase {
    dat: PARENT_CLONE_DAT,
    ..DEFAULT
};
/// Default cell on the categories dat.
const CATEGORIES: SortCase<'static> = SortCase {
    dat: CATEGORIES_DAT,
    ..DEFAULT
};
/// Default cell on the revisions dat.
const REVISIONS: SortCase<'static> = SortCase {
    dat: REVISIONS_DAT,
    ..DEFAULT
};
/// Default cell on the language test dat.
const LANGUAGE: SortCase<'static> = SortCase {
    dat: LANGUAGE_DAT,
    ..DEFAULT
};

async fn run_sort_case(case: &SortCase<'_>) {
    let mut env = TestEnv::with_dat(case.dat).await;

    for rom in case.roms {
        let path = env.fixture(rom).await;
        env.import_roms(&[Path::new(&path)]).await;
    }

    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

    for setting in case.settings {
        match setting {
            Setting::Bool(key, value) => env.set_bool(key, *value, system.id).await,
            Setting::Str(key, value) => env.set_string(key, value, system.id).await,
            Setting::List(key, values) => {
                for value in *values {
                    env.add_to_list(key, value, system.id).await;
                }
            }
        }
    }

    let mut argv = vec!["sort-roms", "--all", "--yes"];
    argv.extend_from_slice(case.args);
    env.run(&argv).await.unwrap();

    let romfiles = env.romfiles_of(&system).await;
    assert_eq!(
        romfiles.len(),
        case.expected.len(),
        "the sort should have left every romfile accounted for"
    );

    for (name, folders) in case.expected {
        let romfile = romfiles
            .iter()
            .find(|romfile| romfile.path.ends_with(name))
            .unwrap_or_else(|| panic!("\"{name}\" should still have a romfile"));
        let mut path = system_directory.join(folders.join("/"));
        path = path.join(name);
        assert_eq!(romfile.path, env.stored_path(&path), "\"{name}\" position");
        assert!(
            env.rom_directory.join(&romfile.path).is_file(),
            "\"{name}\" should be on disk"
        );
    }
}

test_cases! {
    run_sort_case;
    // Sort all, without regions: everything stays where it is.
    sort: SortCase {
        roms: &[
            "Test Game (Asia).rom",
            "Test Game (Japan).rom",
            "Test Game (USA, Europe) (Beta).rom",
            "Test Game (USA, Europe).rom",
        ],
        expected: &[
            ("Test Game (Asia).rom", &[]),
            ("Test Game (Japan).rom", &[]),
            ("Test Game (USA, Europe) (Beta).rom", &[]),
            ("Test Game (USA, Europe).rom", &[]),
        ],
        ..DEFAULT
    },
    // 1G1R, with and without clone knowledge.
    sort_1g1r: SortCase {
        roms: &[
            "Test Game (USA, Europe).rom",
            "Test Game (Asia).rom",
            "Test Game (Japan).rom",
            "Test Game (USA, Europe) (Beta).rom",
        ],
        args: &["--1g1r", "US", "EU"],
        expected: &[
            ("Test Game (USA, Europe).rom", &["1G1R"]),
            ("Test Game (Asia).rom", &["Trash"]),
            ("Test Game (Japan).rom", &["Trash"]),
            ("Test Game (USA, Europe) (Beta).rom", &["Trash"]),
        ],
        ..PARENT_CLONE
    },
    sort_1g1r_infer_clones: SortCase {
        roms: &[
            "Test Game (USA, Europe).rom",
            "Test Game (Asia).rom",
            "Test Game (Japan).rom",
            "Test Game (USA, Europe) (Beta).rom",
        ],
        settings: &[Setting::Bool("INFER_CLONES", true)],
        args: &["--1g1r", "US", "EU"],
        expected: &[
            ("Test Game (USA, Europe).rom", &["1G1R"]),
            ("Test Game (Asia).rom", &["Trash"]),
            ("Test Game (Japan).rom", &["Trash"]),
            ("Test Game (USA, Europe) (Beta).rom", &["Trash"]),
        ],
        ..DEFAULT
    },
    sort_1g1r_without_parent_clone: SortCase {
        roms: &[
            "Test Game (USA, Europe) (Beta).rom",
            "Test Game (USA, Europe).rom",
            "Test Game (Asia).rom",
            "Test Game (Japan).rom",
        ],
        args: &["--1g1r", "US", "EU"],
        expected: &[
            ("Test Game (USA, Europe) (Beta).rom", &["1G1R"]),
            ("Test Game (USA, Europe).rom", &["1G1R"]),
            ("Test Game (Asia).rom", &["Trash"]),
            ("Test Game (Japan).rom", &["Trash"]),
        ],
        ..DEFAULT
    },
    sort_1g1r_lenient: SortCase {
        roms: &[
            "Test Game (USA, Europe) (Beta).rom",
            "Test Game (Asia).rom",
            "Test Game (Japan).rom",
        ],
        args: &["--1g1r", "US", "EU"],
        expected: &[
            ("Test Game (USA, Europe) (Beta).rom", &["1G1R"]),
            ("Test Game (Asia).rom", &["Trash"]),
            ("Test Game (Japan).rom", &["Trash"]),
        ],
        ..PARENT_CLONE
    },
    sort_1g1r_strict: SortCase {
        roms: &[
            "Test Game (Asia).rom",
            "Test Game (Japan).rom",
            "Test Game (USA, Europe) (Beta).rom",
        ],
        settings: &[Setting::Bool("REGIONS_ONE_STRICT", true)],
        args: &["--1g1r", "US", "EU"],
        expected: &[
            ("Test Game (Asia).rom", &["Trash"]),
            ("Test Game (Japan).rom", &["Trash"]),
            ("Test Game (USA, Europe) (Beta).rom", &["Trash"]),
        ],
        ..PARENT_CLONE
    },
    sort_1g1r_revisions: SortCase {
        roms: &[
            "Test Game (USA, Europe).rom",
            "Test Game (Japan).rom",
            "Test Game (USA) (Rev 1).rom",
            "Test Game (USA, Europe) (Beta).rom",
        ],
        settings: &[Setting::Str("PREFER_VERSIONS", "none")],
        args: &["--1g1r", "US", "EU"],
        expected: &[
            ("Test Game (USA, Europe).rom", &["1G1R"]),
            ("Test Game (Japan).rom", &["Trash"]),
            ("Test Game (USA) (Rev 1).rom", &["Trash"]),
            ("Test Game (USA, Europe) (Beta).rom", &["Trash"]),
        ],
        ..REVISIONS
    },
    // Discards, with and without 1G1R.
    sort_1g1r_discard_asia_and_beta: SortCase {
        roms: &[
            "Test Game (USA, Europe).rom",
            "Test Game (Japan).rom",
            "Test Game (Asia).rom",
            "Test Game (USA, Europe) (Beta).rom",
        ],
        settings: &[Setting::List("DISCARD_RELEASES", &["Beta"])],
        args: &["--regions", "JP", "--1g1r", "US", "EU"],
        expected: &[
            ("Test Game (USA, Europe).rom", &["1G1R"]),
            ("Test Game (Japan).rom", &[]),
            ("Test Game (Asia).rom", &["Trash"]),
            ("Test Game (USA, Europe) (Beta).rom", &["Trash"]),
        ],
        ..PARENT_CLONE
    },
    sort_1g1r_discard_categories: SortCase {
        roms: &[
            "Test Game (USA, Europe).rom",
            "Test Game (Japan).rom",
            "Test Game (Asia).rom",
            "Test Game (USA, Europe) (Beta).rom",
        ],
        settings: &[Setting::List("DISCARD_CATEGORIES", &["Demos"])],
        args: &["--regions", "JP", "--1g1r", "AS", "US", "EU"],
        expected: &[
            ("Test Game (USA, Europe).rom", &["1G1R"]),
            ("Test Game (Japan).rom", &[]),
            ("Test Game (Asia).rom", &["Trash"]),
            ("Test Game (USA, Europe) (Beta).rom", &["Trash"]),
        ],
        ..CATEGORIES
    },
    sort_discard_asia: SortCase {
        roms: &[
            "Test Game (Japan).rom",
            "Test Game (USA, Europe) (Beta).rom",
            "Test Game (USA, Europe).rom",
            "Test Game (Asia).rom",
        ],
        args: &["--regions", "US", "EU", "JP"],
        expected: &[
            ("Test Game (Japan).rom", &[]),
            ("Test Game (USA, Europe) (Beta).rom", &[]),
            ("Test Game (USA, Europe).rom", &[]),
            ("Test Game (Asia).rom", &["Trash"]),
        ],
        ..DEFAULT
    },
    sort_discard_asia_and_beta: SortCase {
        roms: &[
            "Test Game (Japan).rom",
            "Test Game (USA, Europe).rom",
            "Test Game (Asia).rom",
            "Test Game (USA, Europe) (Beta).rom",
        ],
        settings: &[Setting::List("DISCARD_RELEASES", &["Beta"])],
        args: &["--regions", "US", "EU", "JP"],
        expected: &[
            ("Test Game (Japan).rom", &[]),
            ("Test Game (USA, Europe).rom", &[]),
            ("Test Game (Asia).rom", &["Trash"]),
            ("Test Game (USA, Europe) (Beta).rom", &["Trash"]),
        ],
        ..DEFAULT
    },
    sort_discard_beta: SortCase {
        roms: &[
            "Test Game (Asia).rom",
            "Test Game (Japan).rom",
            "Test Game (USA, Europe).rom",
            "Test Game (USA, Europe) (Beta).rom",
        ],
        settings: &[Setting::List("DISCARD_RELEASES", &["Beta"])],
        expected: &[
            ("Test Game (Asia).rom", &[]),
            ("Test Game (Japan).rom", &[]),
            ("Test Game (USA, Europe).rom", &[]),
            ("Test Game (USA, Europe) (Beta).rom", &["Trash"]),
        ],
        ..DEFAULT
    },
    sort_discard_categories: SortCase {
        roms: &[
            "Test Game (Japan).rom",
            "Test Game (USA, Europe).rom",
            "Test Game (Asia).rom",
            "Test Game (USA, Europe) (Beta).rom",
        ],
        settings: &[Setting::List("DISCARD_CATEGORIES", &["Demos", "Preproduction"])],
        args: &["--regions", "US", "EU", "JP", "AS"],
        expected: &[
            ("Test Game (Japan).rom", &[]),
            ("Test Game (USA, Europe).rom", &[]),
            ("Test Game (Asia).rom", &["Trash"]),
            ("Test Game (USA, Europe) (Beta).rom", &["Trash"]),
        ],
        ..CATEGORIES
    },
    // Language-first ranking.
    sort_1g1r_language_first: SortCase {
        roms: &[
            "Language Game (USA).rom",
            "Language Game (Europe) (En,Fr).rom",
        ],
        settings: &[
            Setting::Bool("INFER_CLONES", true),
            Setting::Bool("LANGUAGE_FIRST", true),
            Setting::List("PREFERRED_LANGUAGES", &["Fr"]),
        ],
        args: &["--1g1r", "US", "EU"],
        expected: &[
            ("Language Game (Europe) (En,Fr).rom", &["1G1R"]),
            ("Language Game (USA).rom", &["Trash"]),
        ],
        ..LANGUAGE
    },
    sort_1g1r_language_first_demote_not_discard: SortCase {
        roms: &["Other Game (Europe) (De).rom"],
        settings: &[
            Setting::Bool("INFER_CLONES", true),
            Setting::Bool("LANGUAGE_FIRST", true),
            Setting::List("LANGUAGES", &["Fr"]),
            Setting::List("PREFERRED_LANGUAGES", &["Fr"]),
        ],
        args: &["--1g1r", "EU"],
        expected: &[("Other Game (Europe) (De).rom", &["1G1R"])],
        ..LANGUAGE
    },
    sort_1g1r_language_first_empty: SortCase {
        roms: &[
            "Language Game (USA).rom",
            "Language Game (Europe) (En,Fr).rom",
        ],
        settings: &[
            Setting::Bool("INFER_CLONES", true),
            Setting::Bool("LANGUAGE_FIRST", true),
            Setting::List("LANGUAGES", &["Fr"]),
        ],
        args: &["--1g1r", "US", "EU"],
        expected: &[
            ("Language Game (USA).rom", &["1G1R"]),
            ("Language Game (Europe) (En,Fr).rom", &["Trash"]),
        ],
        ..LANGUAGE
    },
    sort_1g1r_language_first_off: SortCase {
        roms: &[
            "Language Game (USA).rom",
            "Language Game (Europe) (En,Fr).rom",
        ],
        settings: &[
            Setting::Bool("INFER_CLONES", true),
            Setting::List("PREFERRED_LANGUAGES", &["Fr"]),
        ],
        args: &["--1g1r", "US", "EU"],
        expected: &[
            ("Language Game (USA).rom", &["1G1R"]),
            ("Language Game (Europe) (En,Fr).rom", &["Trash"]),
        ],
        ..LANGUAGE
    },
    sort_1g1r_language_first_region_tiebreak: SortCase {
        roms: &[
            "Language Game (France) (Fr).rom",
            "Language Game (Europe) (En,Fr).rom",
        ],
        settings: &[
            Setting::Bool("INFER_CLONES", true),
            Setting::Bool("LANGUAGE_FIRST", true),
            Setting::List("PREFERRED_LANGUAGES", &["Fr"]),
        ],
        args: &["--1g1r", "FR", "EU"],
        expected: &[
            ("Language Game (France) (Fr).rom", &["1G1R"]),
            ("Language Game (Europe) (En,Fr).rom", &["Trash"]),
        ],
        ..LANGUAGE
    },
    sort_1g1r_language_first_revisions: SortCase {
        roms: &[
            "Revision Game (USA) (En).rom",
            "Revision Game (USA) (Rev 1) (En).rom",
        ],
        settings: &[
            Setting::Bool("INFER_CLONES", true),
            Setting::Bool("LANGUAGE_FIRST", true),
            Setting::Str("PREFER_VERSIONS", "new"),
            Setting::List("PREFERRED_LANGUAGES", &["En"]),
        ],
        args: &["--1g1r", "US"],
        expected: &[
            ("Revision Game (USA) (En).rom", &["Trash"]),
            ("Revision Game (USA) (Rev 1) (En).rom", &["1G1R"]),
        ],
        ..LANGUAGE
    },
}
