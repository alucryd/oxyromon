use super::*;
use crate::config::{PreferredRegion, PreferredVersion};
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20230105) (Multiple Discs).dat").await;

    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

    let mut romfile_names: Vec<String> = vec![];
    for i in 1..=2 {
        romfile_names.push(format!("Test Game (USA, Europe) (Disc {}).iso", i));
    }

    for romfile_name in &romfile_names {
        let romfile_path = env.tmp_directory.join(romfile_name);
        fs::copy(env.fixture_source(romfile_name), &romfile_path)
            .await
            .unwrap();
        env.import_roms(&[&romfile_path]).await;
    }

    env.run(&["generate-playlists", "-a"]).await.unwrap();

    let all_regions = vec![];
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
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 3);

    let roms_indices = vec![0, 1];

    for i in roms_indices {
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

    let romfile = romfiles.get(2).unwrap();
    assert_eq!(
        &system_directory
            .join("1G1R")
            .join("Test Game (USA, Europe).m3u")
            .strip_prefix(&env.rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
        &romfile.path
    );
    assert!(env.rom_directory.join(&romfile.path).is_file());
}
