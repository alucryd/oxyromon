use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721) (Parent-Clone).dat").await;

    let romfile_names = vec![
        "Test Game (USA, Europe).rom",
        "Test Game (Asia).rom",
        "Test Game (Japan).rom",
        "Test Game (USA, Europe) (Beta).rom",
    ];
    for romfile_name in &romfile_names {
        let romfile_path = env.tmp_directory.join(romfile_name);
        fs::copy(env.fixture_source(romfile_name), &romfile_path)
            .await
            .unwrap();
        env.import_roms(&[&romfile_path]).await;
    }

    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

    let all_regions = vec![Region::Unknown];
    let one_regions = vec![Region::UnitedStates, Region::Europe];

    // when
    sort_system(
        &mut env.connection,
        &env.progress_bar,
        true,
        false,
        &system,
        &all_regions,
        &one_regions,
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        true,
        &PreferredRegion::None,
        &PreferredVersion::None,
        &[],
        &None,
        &None,
        false,
        false,
        false,
        &[],
    )
    .await
    .unwrap();

    // then
    let romfiles = find_romfiles_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(4, romfiles.len());

    let one_regions_indices = vec![0];
    let all_regions_indices = vec![1, 2, 3];

    for i in one_regions_indices {
        let romfile = romfiles.get(i).unwrap();
        assert_eq!(
            &system_directory
                .join("1G1R")
                .join(romfile_names.get(i).unwrap())
                .strip_prefix(&env.rom_directory)
                .unwrap()
                .as_os_str()
                .to_str()
                .unwrap(),
            &romfile.path
        );
        assert!(env.rom_directory.join(&romfile.path).is_file());
    }

    for i in all_regions_indices {
        let romfile = romfiles.get(i).unwrap();
        assert_eq!(
            &system_directory
                .join(romfile_names.get(i).unwrap())
                .strip_prefix(&env.rom_directory)
                .unwrap()
                .as_os_str()
                .to_str()
                .unwrap(),
            &romfile.path
        );
        assert!(env.rom_directory.join(&romfile.path).is_file());
    }
}
