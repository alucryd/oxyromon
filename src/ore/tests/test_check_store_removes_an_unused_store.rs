use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given a system's only game in its store
    let mut env = TestEnv::with_dat("Test System (20261114) (Game Boy).dat").await;
    let romfile_path = env.fixture("Test Game (USA, Europe).gb.7z").await;
    env.import_roms(&[&romfile_path]).await;
    let system = env.system().await;
    let store = system_store(&mut env.connection, &system).await.unwrap();
    let ore = store.with_file_name("Test Game (USA, Europe).ore");
    set_bool(&mut env.connection, "ORE_STORE", true, Some(system.id)).await;
    env.run(&["convert-roms", "-f", "ORE", "-a"]).await.unwrap();

    // when the thin ORE is damaged past reading, as a check leaves it in
    // the Trash beneath the store's directory
    let mut bytes = std::fs::read(&ore).unwrap();
    let meta = u64::from_be_bytes(bytes[72..80].try_into().unwrap()) as usize;
    bytes[meta + 3] ^= 0x40;
    let trash = store.with_file_name("Trash");
    std::fs::create_dir(&trash).unwrap();
    std::fs::write(trash.join("Test Game (USA, Europe).ore"), &bytes).unwrap();
    std::fs::remove_file(&ore).unwrap();
    check_store(&env.progress_bar, &store).await.unwrap();

    // then the store is kept: the ORE may still need its chunks
    assert!(store.is_dir());

    // and once nothing beneath it is left that may use it, it is deleted
    std::fs::remove_dir_all(&trash).unwrap();
    check_store(&env.progress_bar, &store).await.unwrap();
    assert!(!store.exists());
}
