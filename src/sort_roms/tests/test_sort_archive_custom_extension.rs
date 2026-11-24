use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;

    env.run(&[
        "import-dats",
        "-e",
        "ext",
        env.fixture_source("Test System (20000000).dat")
            .to_str()
            .unwrap(),
    ])
    .await
    .unwrap();

    let romfile_path = env.fixture("Test Game (USA, Europe).rom.7z").await;
    env.import_roms(&[&romfile_path]).await;

    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

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
    let romfiles = find_romfiles_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(1, romfiles.len());

    let romfile = romfiles.first().unwrap();
    assert_eq!(
        &system_directory
            .join("1G1R")
            .join("Test Game (USA, Europe).7z")
            .strip_prefix(&env.rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
        &romfile.path
    );
    assert!(env.rom_directory.join(&romfile.path).is_file());
}
