use super::*;
use crate::test_support::TestEnv;
use std::path::PathBuf;

async fn import_one(env: &mut TestEnv, extra: &[&str]) {
    let romfile_path = env.fixture("Test Game (USA, Europe).rom").await;
    let mut args: Vec<&str> = vec!["import-roms"];
    args.extend(extra);
    args.push(romfile_path.to_str().unwrap());
    env.run(&args).await.unwrap();
}

#[tokio::test]
async fn test_prefer_format_converts_on_import() {
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;
    let system = env.system().await;
    set_string(&mut env.connection, "PREFER_FORMAT", "7Z", Some(system.id)).await;

    import_one(&mut env, &[]).await;

    let romfiles = env.romfiles().await;
    assert_eq!(romfiles.len(), 1);
    let path = PathBuf::from(&romfiles[0].path);
    assert_eq!(path.extension().unwrap(), "7z");
    let expected = env
        .system_directory(&system)
        .await
        .strip_prefix(&env.rom_directory)
        .unwrap()
        .join("Test Game (USA, Europe).7z");
    assert_eq!(path, expected);
    assert!(env.rom_directory.join(&path).is_file());
}

#[tokio::test]
async fn test_no_prefer_format_leaves_roms_untouched() {
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    import_one(&mut env, &[]).await;

    let romfiles = env.romfiles().await;
    assert_eq!(romfiles.len(), 1);
    let path = PathBuf::from(&romfiles[0].path);
    assert_eq!(path.extension().unwrap(), "rom");
}

#[tokio::test]
async fn test_as_is_skips_auto_conversion() {
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;
    let system = env.system().await;
    set_string(&mut env.connection, "PREFER_FORMAT", "7Z", Some(system.id)).await;

    import_one(&mut env, &["--as-is"]).await;

    let romfiles = env.romfiles().await;
    assert_eq!(romfiles.len(), 1);
    let path = PathBuf::from(&romfiles[0].path);
    assert_eq!(path.extension().unwrap(), "rom");
}

#[tokio::test]
async fn test_convert_save_persists_prefer_format() {
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;
    let system = env.system().await;

    import_one(&mut env, &[]).await;

    env.run(&["convert-roms", "--all", "--format", "7Z", "--save"])
        .await
        .unwrap();

    let format = get_string(&mut env.connection, "PREFER_FORMAT", Some(system.id)).await;
    assert_eq!(format.as_deref(), Some("7Z"));

    let romfiles = env.romfiles().await;
    let path = PathBuf::from(&romfiles[0].path);
    assert_eq!(path.extension().unwrap(), "7z");
}

#[tokio::test]
async fn test_arcade_rejects_unsupported_prefer_format() {
    let mut env = TestEnv::with_dat("Test System (20200721) (MAME).dat").await;
    let system = env.system().await;
    assert!(system.arcade);

    set_setting(
        &mut env.connection,
        &env.progress_bar,
        "PREFER_FORMAT",
        "CHD",
        Some(system.id),
    )
    .await
    .unwrap();
    let format = get_string(&mut env.connection, "PREFER_FORMAT", Some(system.id)).await;
    assert_eq!(format, None);

    set_setting(
        &mut env.connection,
        &env.progress_bar,
        "PREFER_FORMAT",
        "ZIP",
        Some(system.id),
    )
    .await
    .unwrap();
    let format = get_string(&mut env.connection, "PREFER_FORMAT", Some(system.id)).await;
    assert_eq!(format.as_deref(), Some("ZIP"));
}
