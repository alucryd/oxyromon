pub use super::*;
use crate::test_support::TestEnv;
use ::test_support::ps3;
use md5::{Digest, Md5};
use sha1::Sha1;

mod test_iso_to_jbfolder;
mod test_jbfolder_missing_update;
mod test_jbfolder_to_iso;
mod test_pup_shared;

pub const GAME: &str = "Test Game (USA)";

/// A PS3 system whose one game is the synthetic disc, its DAT written from
/// it, and its IRD imported: the disc, and where its update was left.
pub async fn setup(env: &mut TestEnv) -> (ps3::Disc, PathBuf) {
    let disc = ps3::disc();
    let dat = env.tmp_directory.join("PS3.dat");
    std::fs::write(
        &dat,
        format!(
            r#"<?xml version="1.0"?>
<datafile>
	<header>
		<name>Sony - PlayStation 3</name>
		<description>Sony - PlayStation 3</description>
		<version>20261127</version>
	</header>
	<game name="{GAME}">
		<description>{GAME}</description>
		<rom name="{GAME}.iso" size="{}" crc="{:08x}" md5="{}" sha1="{}"/>
	</game>
</datafile>
"#,
            disc.iso.len(),
            crc32fast::hash(&disc.iso),
            to_hex(&Md5::digest(&disc.iso)),
            to_hex(&Sha1::digest(&disc.iso)),
        ),
    )
    .unwrap();
    env.run(&["import-dats", dat.to_str().unwrap()])
        .await
        .unwrap();

    let ird = env.tmp_directory.join(format!("{GAME}.ird"));
    std::fs::write(&ird, &disc.ird).unwrap();
    env.run(&[
        "import-irds",
        "-s",
        "Sony - PlayStation 3",
        ird.to_str().unwrap(),
    ])
    .await
    .unwrap();

    let update = env.tmp_directory.join("PS3UPDAT.PUP");
    std::fs::write(&update, disc.file(ps3::UPDATE_PATH)).unwrap();
    (disc, update)
}

/// Import the disc's JB folder, without its update, as most go.
pub async fn import_jbfolder(env: &mut TestEnv, disc: &ps3::Disc) {
    let jb = env.tmp_directory.join(GAME);
    disc.write_jb(&jb);
    env.import_roms(&[&jb]).await;
}

pub async fn game(env: &mut TestEnv) -> Game {
    find_games(&mut env.connection).await.remove(0)
}

/// The game's ROMs with a file, by name, with that file's path.
pub async fn romfiles_by_rom(env: &mut TestEnv) -> HashMap<String, PathBuf> {
    let game = game(env).await;
    let mut out = HashMap::new();
    for rom in find_roms_with_romfile_by_game_id(&mut env.connection, game.id).await {
        let romfile = find_romfile_by_id(&mut env.connection, rom.romfile_id.unwrap()).await;
        out.insert(rom.name, env.romfile_path(&romfile));
    }
    out
}
