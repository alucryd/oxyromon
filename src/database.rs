use super::model::*;
use cfg_if::cfg_if;
use itertools::Itertools;
use sqlx::migrate::Migrator;
use sqlx::prelude::*;
use sqlx::sqlite::{SqliteConnection, SqlitePool, SqlitePoolOptions};
use sqlx::{Acquire, Sqlite, Transaction};
use std::convert::TryFrom;
use std::time::Duration;

static MIGRATOR: Migrator = sqlx::migrate!();

pub async fn establish_connection(url: &str) -> SqlitePool {
    let max_connections: u32;
    let locking_mode: &str;
    cfg_if! {
        if #[cfg(feature = "server")] {
            max_connections = 5;
            locking_mode = "NORMAL";
        } else {
            max_connections = 1;
            locking_mode = "EXCLUSIVE";
        }
    }

    let pool = SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .connect(url)
        .await
        .unwrap_or_else(|_| panic!("Error connecting to {}", url));

    pool.execute(
        format!(
            "
            PRAGMA foreign_keys = ON;
            PRAGMA locking_mode = {};
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA temp_store = MEMORY;
            PRAGMA mmap_size = 30000000000;
            PRAGMA auto_vacuum = INCREMENTAL;
        ",
            locking_mode
        )
        .as_str(),
    )
    .await
    .expect("Failed to setup the database");

    MIGRATOR
        .run(&pool)
        .await
        .expect("Failed to run database migrations");

    pool
}

pub async fn begin_transaction(connection: &mut SqliteConnection) -> Transaction<'_, Sqlite> {
    Acquire::begin(connection)
        .await
        .expect("Failed to begin transaction")
}

pub async fn commit_transaction(transaction: Transaction<'_, Sqlite>) {
    transaction
        .commit()
        .await
        .expect("Failed to commit transaction");
}

pub async fn rollback_transaction(transaction: Transaction<'_, Sqlite>) {
    transaction
        .rollback()
        .await
        .expect("Failed to rollback transaction");
}

pub async fn close_connection(pool: &SqlitePool) {
    pool.execute(
        "
            PRAGMA incremental_vacuum;
            PRAGMA optimize;
            PRAGMA wal_checkpoint(truncate);
        ",
    )
    .await
    .expect("Failed to optimize the database");
}

pub async fn create_system_from_xml(
    connection: &mut SqliteConnection,
    system_xml: &SystemXml,
    arcade: bool,
) -> i64 {
    let name = system_xml.name.replace(" (Parent-Clone)", "");
    sqlx::query!(
        "
        INSERT INTO systems (name, description, version, url, arcade)
        VALUES (?, ?, ?, ?, ?)
        ",
        name,
        system_xml.description,
        system_xml.version,
        system_xml.url,
        arcade,
    )
    .execute(connection)
    .await
    .expect("Error while creating system")
    .last_insert_rowid()
}

pub async fn update_system_from_xml(
    connection: &mut SqliteConnection,
    id: i64,
    system_xml: &SystemXml,
    arcade: bool,
) {
    let name = system_xml.name.replace(" (Parent-Clone)", "");
    sqlx::query!(
        "
        UPDATE systems
        SET name = ?, description = ?, version = ?, url = ?, arcade = ?
        WHERE id = ?
        ",
        name,
        system_xml.description,
        system_xml.version,
        system_xml.url,
        arcade,
        id,
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while updating system with id {}", id));
}

pub async fn update_system_mark_complete(connection: &mut SqliteConnection, id: i64) {
    sqlx::query!(
        "
        UPDATE systems
        SET complete = true
        WHERE id = ?
        AND complete = false
        AND NOT EXISTS (
            SELECT g.id
            FROM games g
            WHERE g.system_id = systems.id
            AND g.complete = false
            AND g.sorting != 2
        )
        ",
        id,
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while marking system with id {} as complete", id));
}

pub async fn update_system_mark_incomplete(connection: &mut SqliteConnection, id: i64) {
    sqlx::query!(
        "
        UPDATE systems
        SET complete = false
        WHERE id = ?
        AND complete = true
        AND EXISTS (
            SELECT g.id
            FROM games g
            WHERE g.system_id = systems.id
            AND g.complete = false
            AND g.sorting != 2
        )
        ",
        id,
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while marking system with id {} as incomplete", id));
}

pub async fn update_systems_mark_incomplete(connection: &mut SqliteConnection) {
    sqlx::query!(
        "
        UPDATE systems
        SET complete = false
        WHERE complete = true
        AND EXISTS (
            SELECT g.id
            FROM games g
            WHERE g.system_id = systems.id
            AND g.complete = false
            AND g.sorting != 2
        )
        ",
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while marking systems as incomplete"));
}

pub async fn update_system_merging(connection: &mut SqliteConnection, id: i64, merging: Merging) {
    let merging = merging as i8;
    sqlx::query!(
        "
        UPDATE systems
        SET merging = ?
        WHERE id = ?
        ",
        merging,
        id,
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while updating system with id {} merging", merging));
}

pub async fn find_systems(connection: &mut SqliteConnection) -> Vec<System> {
    sqlx::query_as!(
        System,
        "
        SELECT *
        FROM systems
        ORDER BY name
        ",
    )
    .fetch_all(connection)
    .await
    .expect("Error while finding systems")
}

pub async fn find_arcade_systems(connection: &mut SqliteConnection) -> Vec<System> {
    sqlx::query_as!(
        System,
        "
        SELECT *
        FROM systems
        WHERE arcade = true
        ORDER BY name
        ",
    )
    .fetch_all(connection)
    .await
    .expect("Error while finding arcade systems")
}

pub async fn find_systems_by_url(connection: &mut SqliteConnection, url: &str) -> Vec<System> {
    sqlx::query_as!(
        System,
        "
        SELECT *
        FROM systems
        WHERE url = ?
        ORDER BY name
        ",
        url,
    )
    .fetch_all(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while finding systems with url {}", url))
}

#[cfg(feature = "ird")]
pub async fn find_systems_by_name_like(
    connection: &mut SqliteConnection,
    name: &str,
) -> Vec<System> {
    sqlx::query_as!(
        System,
        "
        SELECT *
        FROM systems
        WHERE name LIKE ?
        ",
        name,
    )
    .fetch_all(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while finding system with name {}", name))
}

pub async fn find_system_by_id(connection: &mut SqliteConnection, id: i64) -> System {
    sqlx::query_as!(
        System,
        "
        SELECT *
        FROM systems
        WHERE id = ?
        ",
        id,
    )
    .fetch_one(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while finding system with id {}", id))
}

pub async fn find_system_by_name(connection: &mut SqliteConnection, name: &str) -> Option<System> {
    let name = name.replace(" (Parent-Clone)", "");
    sqlx::query_as!(
        System,
        "
        SELECT *
        FROM systems
        WHERE name = ?
        ",
        name,
    )
    .fetch_optional(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while finding system with name {}", name))
}

pub async fn delete_system_by_id(connection: &mut SqliteConnection, id: i64) {
    sqlx::query!(
        "
        DELETE FROM systems
        WHERE id = ?
        ",
        id,
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while deleting system with id {}", id));
}

pub async fn create_game_from_xml(
    connection: &mut SqliteConnection,
    game_xml: &GameXml,
    regions: &str,
    system_id: i64,
    parent_id: Option<i64>,
    bios_id: Option<i64>,
) -> i64 {
    let bios = game_xml.isbios.is_some() && game_xml.isbios.as_ref().unwrap() == "yes";
    sqlx::query!(
        "
        INSERT INTO games (name, description, comment, bios, regions, system_id, parent_id, bios_id)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ",
        game_xml.name,
        game_xml.description,
        game_xml.comment,
        bios,
        regions,
        system_id,
        parent_id,
        bios_id,
    )
    .execute(connection)
    .await
    .expect(&format!(
        "Error while creating game with name '{}'",
        game_xml.name
    ))
    .last_insert_rowid()
}

pub async fn update_game_from_xml(
    connection: &mut SqliteConnection,
    id: i64,
    game_xml: &GameXml,
    regions: &str,
    system_id: i64,
    parent_id: Option<i64>,
    bios_id: Option<i64>,
) {
    let bios = game_xml.isbios.is_some() && game_xml.isbios.as_ref().unwrap() == "yes";
    sqlx::query!(
        "
        UPDATE games
        SET name = ?, description = ?, comment = ?, bios = ?, regions = ?, system_id = ?, parent_id = ?, bios_id = ?
        WHERE id = ?
        ",
        game_xml.name,
        game_xml.description,
        game_xml.comment,
        bios,
        regions,
        system_id,
        parent_id,
        bios_id,
        id,
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while updating game with id {}", id));
}

pub async fn update_games_by_system_id_mark_complete(
    connection: &mut SqliteConnection,
    system_id: i64,
) {
    sqlx::query!(
        "
        UPDATE games
        SET complete = true
        WHERE system_id = ?
        AND complete = false
        AND jbfolder = false
        AND NOT EXISTS (
            SELECT r.id
            FROM roms r
            WHERE r.game_id = games.id
            AND r.romfile_id IS NULL
        )
        ",
        system_id,
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while marking games as complete"));
}

#[cfg(feature = "ird")]
pub async fn update_jbfolder_games_by_system_id_mark_complete(
    connection: &mut SqliteConnection,
    system_id: i64,
) {
    sqlx::query!(
        "
        UPDATE games
        SET complete = true
        WHERE system_id = ?
        AND complete = false
        AND jbfolder = true
        AND NOT EXISTS (
            SELECT r.id
            FROM roms r
            WHERE r.game_id = games.id
            AND r.romfile_id IS NULL
            AND r.parent_id IS NOT NULL
            AND r.name NOT LIKE 'PS3_CONTENT/%'
            AND r.name NOT LIKE 'PS3_EXTRA/%'
            AND r.name NOT LIKE 'PS3_UPDATE/%'
        )
        ",
        system_id,
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while marking game as complete"));
}

pub async fn update_games_by_system_id_mark_incomplete(
    connection: &mut SqliteConnection,
    system_id: i64,
) {
    sqlx::query!(
        "
        UPDATE games
        SET complete = false
        WHERE system_id = ?
        AND complete = true
        AND jbfolder = false
        AND EXISTS (
            SELECT r.id
            FROM roms r
            WHERE r.game_id = games.id
            AND r.romfile_id IS NULL
        )
        ",
        system_id,
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while marking games as incomplete"));
}

#[cfg(feature = "ird")]
pub async fn update_jbfolder_games_by_system_id_mark_incomplete(
    connection: &mut SqliteConnection,
    system_id: i64,
) {
    sqlx::query!(
        "
        UPDATE games
        SET complete = false
        WHERE system_id = ?
        AND complete = true
        AND jbfolder = true
        AND EXISTS (
            SELECT r.id
            FROM roms r
            WHERE r.game_id = games.id
            AND r.romfile_id IS NULL
            AND r.parent_id IS NOT NULL
            AND r.name NOT LIKE 'PS3_CONTENT/%'
            AND r.name NOT LIKE 'PS3_EXTRA/%'
            AND r.name NOT LIKE 'PS3_UPDATE/%'
        )
        ",
        system_id,
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while marking games as incomplete"));
}

pub async fn update_games_mark_incomplete(connection: &mut SqliteConnection) {
    sqlx::query!(
        "
        UPDATE games
        SET complete = false
        WHERE complete = true
        AND jbfolder = false
        AND EXISTS (
            SELECT r.id
            FROM roms r
            WHERE r.game_id = games.id
            AND r.romfile_id IS NULL
        )
        ",
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while marking games as incomplete"));
}

pub async fn update_games_sorting(
    connection: &mut SqliteConnection,
    ids: &[i64],
    sorting: Sorting,
) -> u64 {
    let sorting = sorting as i8;
    let sql = format!(
        "
        UPDATE games
        SET sorting = {}
        WHERE id IN ({})
        AND sorting != {}
        ",
        sorting,
        ids.iter().join(","),
        sorting,
    );
    sqlx::query(&sql)
        .execute(connection)
        .await
        .unwrap_or_else(|_| panic!("Error while updating games sorting"))
        .rows_affected()
}

#[cfg(feature = "ird")]
pub async fn update_game_jbfolder(connection: &mut SqliteConnection, id: i64, jbfolder: bool) {
    sqlx::query!(
        "
        UPDATE games
        SET jbfolder = ?
        WHERE id = ?
        ",
        jbfolder,
        id,
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while updating game with id {}", id));
}

pub async fn update_game_playlist(connection: &mut SqliteConnection, id: i64, playlist_id: i64) {
    sqlx::query!(
        "
        UPDATE games
        SET playlist_id = ?
        WHERE id = ?
        ",
        playlist_id,
        id,
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while updating game with id {}", id));
}

pub async fn find_games(connection: &mut SqliteConnection) -> Vec<Game> {
    sqlx::query_as!(
        Game,
        "
        SELECT *
        FROM games
        ORDER BY name
        ",
    )
    .fetch_all(connection)
    .await
    .expect("Error while finding games")
}

pub async fn find_games_by_system_id(
    connection: &mut SqliteConnection,
    system_id: i64,
) -> Vec<Game> {
    sqlx::query_as!(
        Game,
        "
        SELECT *
        FROM games
        WHERE system_id = ?
        ORDER BY name
        ",
        system_id,
    )
    .fetch_all(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while finding games with system id {}", system_id))
}

#[cfg(feature = "ird")]
pub async fn find_wanted_games_by_system_id(
    connection: &mut SqliteConnection,
    system_id: i64,
) -> Vec<Game> {
    sqlx::query_as!(
        Game,
        "
        SELECT *
        FROM games
        WHERE system_id = ?
        AND sorting != 2 
        ORDER BY name
        ",
        system_id,
    )
    .fetch_all(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while finding games with system id {}", system_id))
}

#[cfg(test)]
pub async fn find_games_by_ids(connection: &mut SqliteConnection, ids: &[i64]) -> Vec<Game> {
    let sql = format!(
        "
        SELECT *
        FROM games
        WHERE id IN ({})
        ORDER BY name
        ",
        ids.iter().join(",")
    );
    sqlx::query_as::<_, Game>(&sql)
        .fetch_all(connection)
        .await
        .expect("Error while finding games")
}

pub async fn find_parent_games_by_system_id(
    connection: &mut SqliteConnection,
    system_id: i64,
) -> Vec<Game> {
    sqlx::query_as!(
        Game,
        "
        SELECT *
        FROM games
        WHERE system_id = ?
        AND parent_id IS NULL
        ORDER BY name
        ",
        system_id,
    )
    .fetch_all(connection)
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Error while finding parent games with system id {}",
            system_id
        )
    })
}

pub async fn find_clone_games_by_system_id(
    connection: &mut SqliteConnection,
    system_id: i64,
) -> Vec<Game> {
    sqlx::query_as!(
        Game,
        "
        SELECT *
        FROM games
        WHERE system_id = ?
        AND parent_id IS NOT NULL
        ORDER BY name
        ",
        system_id,
    )
    .fetch_all(connection)
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Error while finding parent games with system id {}",
            system_id
        )
    })
}

pub async fn find_game_by_id(connection: &mut SqliteConnection, id: i64) -> Game {
    sqlx::query_as!(
        Game,
        "
        SELECT *
        FROM games
        WHERE id = ?
        ",
        id,
    )
    .fetch_one(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while finding game with id {}", id))
}

pub async fn find_game_by_name_and_bios_and_system_id(
    connection: &mut SqliteConnection,
    name: &str,
    bios: bool,
    system_id: i64,
) -> Option<Game> {
    sqlx::query_as!(
        Game,
        "
        SELECT *
        FROM games
        WHERE name = ?
        AND bios = ?
        AND system_id = ?
        ",
        name,
        bios,
        system_id,
    )
    .fetch_optional(connection)
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Error while finding games with name {} and system id {}",
            name, system_id
        )
    })
}

pub async fn find_games_with_romfiles_by_system_id(
    connection: &mut SqliteConnection,
    system_id: i64,
) -> Vec<Game> {
    sqlx::query_as!(
        Game,
        "
        SELECT *
        FROM games
        WHERE system_id = ?
        AND id IN (
            SELECT DISTINCT(game_id)
            FROM roms
            WHERE romfile_id IS NOT NULL
        )
        ORDER BY name
        ",
        system_id,
    )
    .fetch_all(connection)
    .await
    .expect("Error while finding games with romfiles")
}

pub async fn find_games_with_romfiles_by_name_and_system_id(
    connection: &mut SqliteConnection,
    name: &str,
    system_id: i64,
) -> Vec<Game> {
    sqlx::query_as!(
        Game,
        "
        SELECT *
        FROM games
        WHERE name LIKE ?
        AND system_id = ?
        AND id IN (
            SELECT DISTINCT(game_id)
            FROM roms
            WHERE romfile_id IS NOT NULL
        )
        ORDER BY name
        ",
        name,
        system_id,
    )
    .fetch_all(connection)
    .await
    .expect("Error while finding games with romfiles")
}

pub async fn delete_game_by_name_and_system_id(
    connection: &mut SqliteConnection,
    name: &str,
    system_id: i64,
) {
    sqlx::query!(
        "
        DELETE FROM games
        WHERE name = ?
        AND system_id = ?
        ",
        name,
        system_id,
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Error while deleting game with name {} and system_id {}",
            name, system_id
        )
    });
}

#[cfg(feature = "ird")]
pub async fn create_rom(
    connection: &mut SqliteConnection,
    name: &str,
    size: i64,
    md5: &str,
    game_id: i64,
    parent_id: Option<i64>,
) -> i64 {
    sqlx::query!(
        "
        INSERT INTO roms (name, size, md5, game_id, parent_id)
        VALUES (?, ?, ?, ?, ?)
        ",
        name,
        size,
        md5,
        game_id,
        parent_id,
    )
    .execute(connection)
    .await
    .expect("Error while creating rom")
    .last_insert_rowid()
}

pub async fn create_rom_from_xml(
    connection: &mut SqliteConnection,
    rom_xml: &RomXml,
    bios: bool,
    game_id: i64,
    parent_id: Option<i64>,
) -> i64 {
    if rom_xml.crc.is_none() {
        panic!("Game \"{}\" has no CRC", &rom_xml.name);
    }
    let crc = rom_xml.crc.as_ref().unwrap().to_lowercase();
    let md5 = rom_xml.md5.as_ref().map(|md5| md5.to_lowercase());
    let sha1 = rom_xml.sha1.as_ref().map(|sha1| sha1.to_lowercase());
    sqlx::query!(
        "
        INSERT INTO roms (name, bios, size, crc, md5, sha1, rom_status, game_id, parent_id)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        ",
        rom_xml.name,
        bios,
        rom_xml.size,
        crc,
        md5,
        sha1,
        rom_xml.status,
        game_id,
        parent_id,
    )
    .execute(connection)
    .await
    .expect("Error while creating rom")
    .last_insert_rowid()
}

#[cfg(feature = "ird")]
pub async fn update_rom(
    connection: &mut SqliteConnection,
    id: i64,
    name: &str,
    size: i64,
    md5: &str,
    game_id: i64,
    parent_id: Option<i64>,
) {
    sqlx::query!(
        "
        UPDATE roms
        SET name = ?, size = ?, md5 = ?, game_id = ?, parent_id = ?
        WHERE id = ?
        ",
        name,
        size,
        md5,
        game_id,
        parent_id,
        id,
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while updating rom with id {}", id));
}

pub async fn update_rom_from_xml(
    connection: &mut SqliteConnection,
    id: i64,
    rom_xml: &RomXml,
    bios: bool,
    game_id: i64,
    parent_id: Option<i64>,
) {
    if rom_xml.crc.is_none() {
        panic!("Game \"{}\" has no CRC", &rom_xml.name);
    }
    let crc = rom_xml.crc.as_ref().unwrap().to_lowercase();
    let md5 = rom_xml.md5.as_ref().map(|md5| md5.to_lowercase());
    let sha1 = rom_xml.sha1.as_ref().map(|sha1| sha1.to_lowercase());
    sqlx::query!(
        "
        UPDATE roms
        SET name = ?, bios = ?, size = ?, crc = ?, md5 = ?, sha1 = ?, rom_status = ?, game_id = ?, parent_id = ?
        WHERE id = ?
        ",
        rom_xml.name,
        bios,
        rom_xml.size,
        crc,
        md5,
        sha1,
        rom_xml.status,
        game_id,
        parent_id,
        id,
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while updating rom with id {}", id));
}

pub async fn update_rom_romfile(
    connection: &mut SqliteConnection,
    id: i64,
    romfile_id: Option<i64>,
) {
    sqlx::query!(
        "
        UPDATE roms
        SET romfile_id = ?
        WHERE id = ?
        ",
        romfile_id,
        id,
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while updating rom with id {}", id));
}

pub async fn find_rom_by_id(connection: &mut SqliteConnection, id: i64) -> Rom {
    sqlx::query_as!(
        Rom,
        "
        SELECT *
        FROM roms
        WHERE id = ?
        ",
        id,
    )
    .fetch_one(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while finding rom with id {}", id))
}

pub async fn find_roms(connection: &mut SqliteConnection) -> Vec<Rom> {
    sqlx::query_as!(
        Rom,
        "
        SELECT *
        FROM roms
        ORDER BY name
        ",
    )
    .fetch_all(connection)
    .await
    .expect("Error while finding roms")
}

pub async fn find_roms_by_romfile_id(
    connection: &mut SqliteConnection,
    romfile_id: i64,
) -> Vec<Rom> {
    sqlx::query_as!(
        Rom,
        "
        SELECT *
        FROM roms
        WHERE romfile_id = ?
        ",
        romfile_id,
    )
    .fetch_all(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while finding roms with romfile id {}", romfile_id))
}

pub async fn find_roms_by_game_id_no_parents(
    connection: &mut SqliteConnection,
    game_id: i64,
) -> Vec<Rom> {
    sqlx::query_as!(
        Rom,
        "
        SELECT *
        FROM roms
        WHERE game_id = ?
        AND parent_id IS NULL
        ORDER BY name
        ",
        game_id,
    )
    .fetch_all(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while finding roms with game id {}", game_id))
}

pub async fn find_roms_by_game_id_parents_no_parent_bioses(
    connection: &mut SqliteConnection,
    game_id: i64,
) -> Vec<Rom> {
    sqlx::query_as!(
        Rom,
        "
        SELECT *
        FROM roms
        WHERE game_id = ?
        AND (
            parent_id IS NULL
            OR (
                parent_id IS NOT NULL
                AND bios = false
            )
        )
        ORDER BY name
        ",
        game_id,
    )
    .fetch_all(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while finding roms with game id {}", game_id))
}

pub async fn find_roms_by_game_id_parents(
    connection: &mut SqliteConnection,
    game_id: i64,
) -> Vec<Rom> {
    sqlx::query_as!(
        Rom,
        "
        SELECT *
        FROM roms
        WHERE game_id = ?
        ORDER BY name
        ",
        game_id,
    )
    .fetch_all(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while finding roms with game id {}", game_id))
}

pub async fn find_roms_by_game_id_parents_only(
    connection: &mut SqliteConnection,
    game_id: i64,
) -> Vec<Rom> {
    sqlx::query_as!(
        Rom,
        "
        SELECT *
        FROM roms
        WHERE game_id = ?
        AND parent_id IS NOT NULL
        ORDER BY name
        ",
        game_id,
    )
    .fetch_all(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while finding roms with game id {}", game_id))
}

pub async fn find_roms_by_game_id_parent_bioses_only(
    connection: &mut SqliteConnection,
    game_id: i64,
) -> Vec<Rom> {
    sqlx::query_as!(
        Rom,
        "
        SELECT *
        FROM roms
        WHERE game_id = ?
        AND parent_id IS NOT NULL
        AND bios = true
        ORDER BY name
        ",
        game_id,
    )
    .fetch_all(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while finding roms with game id {}", game_id))
}

pub async fn find_roms_without_romfile_by_game_ids(
    connection: &mut SqliteConnection,
    game_ids: &[i64],
) -> Vec<Rom> {
    let sql = format!(
        "
        SELECT *
        FROM roms
        WHERE romfile_id IS NULL
        AND parent_id IS NULL
        AND game_id IN ({})
        ORDER BY name
        ",
        game_ids.iter().join(", ")
    );
    sqlx::query_as::<_, Rom>(&sql)
        .fetch_all(connection)
        .await
        .expect("Error while finding roms with romfile")
}

pub async fn find_roms_with_romfile_by_system_id(
    connection: &mut SqliteConnection,
    system_id: i64,
) -> Vec<Rom> {
    sqlx::query_as!(
        Rom,
        "
        SELECT r.id, r.name, r.bios, r.size, r.crc, r.md5, r.sha1, r.rom_status, r.game_id, r.romfile_id, r.parent_id
        FROM roms AS r
        JOIN games AS g ON r.game_id = g.id
        WHERE r.romfile_id IS NOT NULL
        AND g.system_id = ?
        ORDER BY r.name
        ",
        system_id,
    )
    .fetch_all(connection)
    .await
    .expect("Error while finding roms with romfile")
}

pub async fn find_roms_with_romfile_by_game_ids(
    connection: &mut SqliteConnection,
    game_ids: &[i64],
) -> Vec<Rom> {
    let sql = format!(
        "
    SELECT *
    FROM roms
    WHERE romfile_id IS NOT NULL
    AND game_id IN ({})
    ORDER BY name
    ",
        game_ids.iter().join(",")
    );
    sqlx::query_as::<_, Rom>(&sql)
        .fetch_all(connection)
        .await
        .expect("Error while finding roms with romfile")
}

pub async fn find_roms_with_romfile_by_size_and_crc_and_system_id(
    connection: &mut SqliteConnection,
    size: i64,
    crc: &str,
    system_id: i64,
) -> Vec<Rom> {
    sqlx::query_as!(
        Rom,
        "
        SELECT r.id, r.name, r.bios, r.size, r.crc, r.md5, r.sha1, r.rom_status, r.game_id, r.romfile_id, r.parent_id
        FROM roms AS r
        JOIN games AS g ON r.game_id = g.id
        WHERE r.romfile_id IS NOT NULL
        AND r.size = ?
        AND r.crc = ?
        AND g.system_id = ?
        ORDER BY r.name
        ",
        size,
        crc,
        system_id
    )
    .fetch_all(connection)
    .await
    .expect("Error while finding roms with romfile")
}

pub async fn find_roms_without_romfile_by_size_and_md5(
    connection: &mut SqliteConnection,
    size: u64,
    md5: &str,
) -> Vec<Rom> {
    let size = i64::try_from(size).unwrap();
    let md5 = md5.to_lowercase();
    sqlx::query_as!(
        Rom,
        "
        SELECT *
        FROM roms
        WHERE romfile_id IS NULL
        AND size = ?
        AND md5 = ?
        ORDER BY name
        ",
        size,
        md5,
    )
    .fetch_all(connection)
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Error while finding roms with size {} and MD5 {}",
            size, md5
        )
    })
}

pub async fn find_roms_without_romfile_by_size_and_md5_and_system_id(
    connection: &mut SqliteConnection,
    size: u64,
    md5: &str,
    system_id: i64,
) -> Vec<Rom> {
    let size = i64::try_from(size).unwrap();
    let md5 = md5.to_lowercase();
    sqlx::query_as!(
        Rom,
        "
        SELECT r.id, r.name, r.bios, r.size, r.crc, r.md5, r.sha1, r.rom_status, r.game_id, r.romfile_id, r.parent_id
        FROM roms AS r
        JOIN games AS g ON r.game_id = g.id
        WHERE r.romfile_id IS NULL
        AND r.size = ?
        AND r.md5 = ?
        AND g.system_id = ?
        ORDER BY r.name
        ",
        size,
        md5,
        system_id,
    )
    .fetch_all(connection)
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Error while finding roms with size {} and MD5 {} and system id {}",
            size, md5, system_id
        )
    })
}

pub async fn find_roms_without_romfile_by_size_and_md5_and_game_names_and_system_id(
    connection: &mut SqliteConnection,
    size: u64,
    md5: &str,
    game_names: &Vec<&str>,
    system_id: i64,
) -> Vec<Rom> {
    let size = i64::try_from(size).unwrap();
    let md5 = md5.to_lowercase();
    let sql = format!(
        "
        SELECT r.id, r.name, r.bios, r.size, r.crc, r.md5, r.sha1, r.rom_status, r.game_id, r.romfile_id, r.parent_id
        FROM roms AS r
        JOIN games AS g ON r.game_id = g.id
        WHERE r.romfile_id IS NULL
        AND r.size = {}
        AND r.md5 = '{}'
        AND g.name IN ({})
        AND g.system_id = {}
        ORDER BY r.name
        ",
        size,
        md5,
        game_names.iter().map(|game_name| format!("'{}'", game_name)).join(","),
        system_id,
    );
    sqlx::query_as::<_, Rom>(&sql)
        .fetch_all(connection)
        .await
        .unwrap_or_else(|_| {
            panic!(
            "Error while finding roms with size {} and MD5 {} and game names {:?} and system id {}",
            size, md5, game_names, system_id
        )
        })
}

pub async fn find_roms_without_romfile_by_name_and_size_and_md5_and_game_names_and_system_id(
    connection: &mut SqliteConnection,
    name: &str,
    size: u64,
    md5: &str,
    game_names: &Vec<&str>,
    system_id: i64,
) -> Vec<Rom> {
    let size = i64::try_from(size).unwrap();
    let md5 = md5.to_lowercase();
    let sql = format!(
        "
        SELECT r.id, r.name, r.bios, r.size, r.crc, r.md5, r.sha1, r.rom_status, r.game_id, r.romfile_id, r.parent_id
        FROM roms AS r
        JOIN games AS g ON r.game_id = g.id
        WHERE r.romfile_id IS NULL
        AND r.name = '{}'
        AND r.size = {}
        AND r.sha1 = '{}'
        AND g.name IN ({})
        AND g.system_id = {}
        ORDER BY r.name
        ",
        name,
        size,
        md5,
        game_names.iter().map(|game_name| format!("'{}'", game_name)).join(","),
        system_id,
    );
    sqlx::query_as::<_, Rom>(&sql)
        .fetch_all(connection)
        .await
        .unwrap_or_else(|_| {
            panic!(
            "Error while finding roms with name {} and size {} and MD5 {} and game names {:?} and system id {}",
            name, size, md5, game_names, system_id
        )
        })
}

pub async fn count_roms_with_romfile_by_size_and_md5(
    connection: &mut SqliteConnection,
    size: u64,
    md5: &str,
) -> i32 {
    let size = i64::try_from(size).unwrap();
    let md5 = md5.to_lowercase();
    sqlx::query!(
        "
        SELECT COUNT(id) AS 'count!'
        FROM roms
        WHERE romfile_id IS NOT NULL
        AND size = ?
        AND md5 = ?
        ",
        size,
        md5,
    )
    .fetch_one(connection)
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Error while finding roms with size {} and MD5 {}",
            size, md5
        )
    })
    .count
}

pub async fn count_roms_with_romfile_by_size_and_md5_and_system_id(
    connection: &mut SqliteConnection,
    size: u64,
    md5: &str,
    system_id: i64,
) -> i32 {
    let size = i64::try_from(size).unwrap();
    let md5 = md5.to_lowercase();
    sqlx::query!(
        "
        SELECT COUNT(r.id) AS 'count!'
        FROM roms AS r
        JOIN games AS g ON r.game_id = g.id
        WHERE r.romfile_id IS NOT NULL
        AND r.size = ?
        AND r.md5 = ?
        AND g.system_id = ?
        ",
        size,
        md5,
        system_id,
    )
    .fetch_one(connection)
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Error while finding roms with size {} and MD5 {} and system id {}",
            size, md5, system_id
        )
    })
    .count
}

pub async fn find_roms_without_romfile_by_size_and_sha1(
    connection: &mut SqliteConnection,
    size: u64,
    sha1: &str,
) -> Vec<Rom> {
    let size = i64::try_from(size).unwrap();
    let sha1 = sha1.to_lowercase();
    sqlx::query_as!(
        Rom,
        "
        SELECT *
        FROM roms
        WHERE romfile_id IS NULL
        AND size = ?
        AND sha1 = ?
        ORDER BY name
        ",
        size,
        sha1,
    )
    .fetch_all(connection)
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Error while finding roms with size {} and SHA1 {}",
            size, sha1
        )
    })
}

pub async fn find_roms_without_romfile_by_size_and_sha1_and_system_id(
    connection: &mut SqliteConnection,
    size: u64,
    sha1: &str,
    system_id: i64,
) -> Vec<Rom> {
    let size = i64::try_from(size).unwrap();
    let sha1 = sha1.to_lowercase();
    sqlx::query_as!(
        Rom,
        "
        SELECT r.id, r.name, r.bios, r.size, r.crc, r.md5, r.sha1, r.rom_status, r.game_id, r.romfile_id, r.parent_id
        FROM roms AS r
        JOIN games AS g ON r.game_id = g.id
        WHERE r.romfile_id IS NULL
        AND r.size = ?
        AND r.sha1 = ?
        AND g.system_id = ?
        ORDER BY r.name
        ",
        size,
        sha1,
        system_id,
    )
    .fetch_all(connection)
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Error while finding roms with size {} and SHA1 {} and system id {}",
            size, sha1, system_id
        )
    })
}

pub async fn find_roms_without_romfile_by_size_and_sha1_and_game_names_and_system_id(
    connection: &mut SqliteConnection,
    size: u64,
    sha1: &str,
    game_names: &Vec<&str>,
    system_id: i64,
) -> Vec<Rom> {
    let size = i64::try_from(size).unwrap();
    let sha1 = sha1.to_lowercase();
    let sql = format!(
        "
        SELECT r.id, r.name, r.bios, r.size, r.crc, r.md5, r.sha1, r.rom_status, r.game_id, r.romfile_id, r.parent_id
        FROM roms AS r
        JOIN games AS g ON r.game_id = g.id
        WHERE r.romfile_id IS NULL
        AND r.size = {}
        AND r.sha1 = '{}'
        AND g.name IN ({})
        AND g.system_id = {}
        ORDER BY r.name
        ",
        size,
        sha1,
        game_names.iter().map(|game_name| format!("'{}'", game_name)).join(","),
        system_id,
    );
    sqlx::query_as::<_, Rom>(&sql)
        .fetch_all(connection)
        .await
        .unwrap_or_else(|_| {
            panic!(
            "Error while finding roms with size {} and SHA1 {} and game names {:?} and system id {}",
            size, sha1, game_names, system_id
        )
    })
}

pub async fn find_roms_without_romfile_by_name_and_size_and_sha1_and_game_names_and_system_id(
    connection: &mut SqliteConnection,
    name: &str,
    size: u64,
    sha1: &str,
    game_names: &Vec<&str>,
    system_id: i64,
) -> Vec<Rom> {
    let size = i64::try_from(size).unwrap();
    let sha1 = sha1.to_lowercase();
    let sql = format!(
        "
        SELECT r.id, r.name, r.bios, r.size, r.crc, r.md5, r.sha1, r.rom_status, r.game_id, r.romfile_id, r.parent_id
        FROM roms AS r
        JOIN games AS g ON r.game_id = g.id
        WHERE r.romfile_id IS NULL
        AND r.name = '{}'
        AND r.size = {}
        AND r.sha1 = '{}'
        AND g.name IN ({})
        AND g.system_id = {}
        ORDER BY r.name
        ",
        name,
        size,
        sha1,
        game_names.iter().map(|game_name| format!("'{}'", game_name)).join(","),
        system_id,
    );
    sqlx::query_as::<_, Rom>(&sql)
        .fetch_all(connection)
        .await
        .unwrap_or_else(|_| {
            panic!(
            "Error while finding roms with name {} and size {} and SHA1 {} and game names {:?} and system id {}",
            name, size, sha1, game_names, system_id
        )
        })
}

pub async fn count_roms_with_romfile_by_size_and_sha1(
    connection: &mut SqliteConnection,
    size: u64,
    sha1: &str,
) -> i32 {
    let size = i64::try_from(size).unwrap();
    let sha1 = sha1.to_lowercase();
    sqlx::query!(
        "
        SELECT COUNT(id) AS 'count!'
        FROM roms
        WHERE romfile_id IS NOT NULL
        AND size = ?
        AND sha1 = ?
        ORDER BY name
        ",
        size,
        sha1,
    )
    .fetch_one(connection)
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Error while finding roms with size {} and SHA1 {}",
            size, sha1
        )
    })
    .count
}

pub async fn count_roms_with_romfile_by_size_and_sha1_and_system_id(
    connection: &mut SqliteConnection,
    size: u64,
    sha1: &str,
    system_id: i64,
) -> i32 {
    let size = i64::try_from(size).unwrap();
    let sha1 = sha1.to_lowercase();
    sqlx::query!(
        "
        SELECT COUNT(r.id) AS 'count!'
        FROM roms AS r
        JOIN games AS g ON r.game_id = g.id
        WHERE r.romfile_id IS NOT NULL
        AND r.size = ?
        AND r.sha1 = ?
        AND g.system_id = ?
        ORDER BY r.name
        ",
        size,
        sha1,
        system_id,
    )
    .fetch_one(connection)
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Error while finding roms with size {} and SHA1 {} and system id {}",
            size, sha1, system_id
        )
    })
    .count
}

pub async fn find_roms_without_romfile_by_size_and_crc(
    connection: &mut SqliteConnection,
    size: u64,
    crc: &str,
) -> Vec<Rom> {
    let size = i64::try_from(size).unwrap();
    let crc = crc.to_lowercase();
    sqlx::query_as!(
        Rom,
        "
        SELECT *
        FROM roms
        WHERE romfile_id IS NULL
        AND size = ?
        AND crc = ?
        ORDER BY name
        ",
        size,
        crc,
    )
    .fetch_all(connection)
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Error while finding roms with size {} and CRC {}",
            size, crc
        )
    })
}

pub async fn find_roms_without_romfile_by_size_and_crc_and_system_id(
    connection: &mut SqliteConnection,
    size: u64,
    crc: &str,
    system_id: i64,
) -> Vec<Rom> {
    let size = i64::try_from(size).unwrap();
    let crc = crc.to_lowercase();
    sqlx::query_as!(
        Rom,
        "
        SELECT r.id, r.name, r.bios, r.size, r.crc, r.md5, r.sha1, r.rom_status, r.game_id, r.romfile_id, r.parent_id
        FROM roms AS r
        JOIN games AS g ON r.game_id = g.id
        WHERE r.romfile_id IS NULL
        AND r.size = ?
        AND r.crc = ?
        AND g.system_id = ?
        ORDER BY r.name
        ",
        size,
        crc,
        system_id,
    )
    .fetch_all(connection)
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Error while finding roms with size {} and CRC {} and system id {}",
            size, crc, system_id
        )
    })
}

pub async fn find_roms_without_romfile_by_size_and_crc_and_game_names_and_system_id(
    connection: &mut SqliteConnection,
    size: u64,
    crc: &str,
    game_names: &Vec<&str>,
    system_id: i64,
) -> Vec<Rom> {
    let size = i64::try_from(size).unwrap();
    let crc = crc.to_lowercase();
    let sql = format!(
        "
        SELECT r.id, r.name, r.bios, r.size, r.crc, r.md5, r.sha1, r.rom_status, r.game_id, r.romfile_id, r.parent_id
        FROM roms AS r
        JOIN games AS g ON r.game_id = g.id
        WHERE r.romfile_id IS NULL
        AND r.size = {}
        AND r.crc = '{}'
        AND g.name IN ({})
        AND g.system_id = {}
        ORDER BY r.name
        ",
        size,
        crc,
        game_names.iter().map(|game_name| format!("'{}'", game_name)).join(","),
        system_id,
    );
    sqlx::query_as::<_, Rom>(&sql)
        .fetch_all(connection)
        .await
        .unwrap_or_else(|_| {
            panic!(
            "Error while finding roms with size {} and CRC {} and game names {:?} and system id {}",
            size, crc, game_names, system_id
        )
        })
}

pub async fn find_roms_without_romfile_by_name_and_size_and_crc_and_game_names_and_system_id(
    connection: &mut SqliteConnection,
    name: &str,
    size: u64,
    crc: &str,
    game_names: &Vec<&str>,
    system_id: i64,
) -> Vec<Rom> {
    let size = i64::try_from(size).unwrap();
    let crc = crc.to_lowercase();
    let sql = format!(
        "
        SELECT r.id, r.name, r.bios, r.size, r.crc, r.md5, r.sha1, r.rom_status, r.game_id, r.romfile_id, r.parent_id
        FROM roms AS r
        JOIN games AS g ON r.game_id = g.id
        WHERE r.romfile_id IS NULL
        AND r.name = '{}'
        AND r.size = {}
        AND r.crc = '{}'
        AND g.name IN ({})
        AND g.system_id = {}
        ORDER BY r.name
        ",
        name,
        size,
        crc,
        game_names.iter().map(|game_name| format!("'{}'", game_name)).join(","),
        system_id,
    );
    sqlx::query_as::<_, Rom>(&sql)
        .fetch_all(connection)
        .await
        .unwrap_or_else(|_| {
            panic!(
            "Error while finding roms with name {} and size {} and CRC {} and game names {:?} and system id {}",
            name, size, crc, game_names, system_id
        )
        })
}

pub async fn count_roms_with_romfile_by_size_and_crc(
    connection: &mut SqliteConnection,
    size: u64,
    crc: &str,
) -> i32 {
    let size = i64::try_from(size).unwrap();
    let crc = crc.to_lowercase();
    sqlx::query!(
        "
        SELECT COUNT(id) AS 'count!'
        FROM roms
        WHERE romfile_id IS NOT NULL
        AND size = ?
        AND crc = ?
        ",
        size,
        crc,
    )
    .fetch_one(connection)
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Error while finding roms with size {} and CRC {}",
            size, crc
        )
    })
    .count
}

pub async fn count_roms_with_romfile_by_size_and_crc_and_system_id(
    connection: &mut SqliteConnection,
    size: u64,
    crc: &str,
    system_id: i64,
) -> i32 {
    let size = i64::try_from(size).unwrap();
    let crc = crc.to_lowercase();
    sqlx::query!(
        "
        SELECT COUNT(r.id) AS 'count!'
        FROM roms AS r
        JOIN games AS g ON r.game_id = g.id
        WHERE r.romfile_id IS NOT NULL
        AND r.size = ?
        AND r.crc = ?
        AND g.system_id = ?
        ",
        size,
        crc,
        system_id,
    )
    .fetch_one(connection)
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Error while finding roms with size {} and CRC {} and system id {}",
            size, crc, system_id
        )
    })
    .count
}

#[cfg(feature = "ird")]
pub async fn find_roms_without_romfile_by_name_and_size_and_md5_and_system_id(
    connection: &mut SqliteConnection,
    name: &str,
    size: u64,
    md5: &str,
    system_id: i64,
) -> Vec<Rom> {
    let size = i64::try_from(size).unwrap();
    let md5 = md5.to_lowercase();
    sqlx::query_as!(
        Rom,
        "
        SELECT r.id, r.name, r.bios, r.size, r.crc, r.md5, r.sha1, r.rom_status, r.game_id, r.romfile_id, r.parent_id
        FROM roms AS r
        JOIN games AS g ON r.game_id = g.id
        WHERE r.romfile_id IS NULL
        AND r.name = ?
        AND r.size = ?
        AND r.md5 = ?
        AND r.parent_id IS NOT NULL
        AND g.system_id = ?
        ORDER BY g.name
        ",
        name,
        size,
        md5,
        system_id,
    )
    .fetch_all(connection)
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Error while finding roms with name {} and size {} and MD5 {} and system id {}",
            name, size, md5, system_id
        )
    })
}

#[cfg(feature = "ird")]
pub async fn count_roms_with_romfile_by_name_and_size_and_md5_and_system_id(
    connection: &mut SqliteConnection,
    name: &str,
    size: u64,
    md5: &str,
    system_id: i64,
) -> i32 {
    let size = i64::try_from(size).unwrap();
    let md5 = md5.to_lowercase();
    sqlx::query!(
        "
        SELECT COUNT(r.id) AS 'count!'
        FROM roms AS r
        JOIN games AS g ON r.game_id = g.id
        WHERE r.romfile_id IS NULL
        AND r.name = ?
        AND r.size = ?
        AND r.md5 = ?
        AND r.parent_id IS NOT NULL
        AND g.system_id = ?
        ",
        name,
        size,
        md5,
        system_id,
    )
    .fetch_one(connection)
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Error while finding roms with name {} and size {} and MD5 {} and system id {}",
            name, size, md5, system_id
        )
    })
    .count
}

#[cfg(feature = "ird")]
pub async fn find_roms_without_romfile_by_size_and_md5_and_parent_id(
    connection: &mut SqliteConnection,
    size: u64,
    md5: &str,
    parent_id: i64,
) -> Vec<Rom> {
    let size = i64::try_from(size).unwrap();
    let md5 = md5.to_lowercase();
    sqlx::query_as!(
        Rom,
        "
        SELECT *
        FROM roms
        WHERE romfile_id IS NULL
        AND size = ?
        AND md5 = ?
        AND parent_id = ?
        ORDER BY name
        ",
        size,
        md5,
        parent_id,
    )
    .fetch_all(connection)
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Error while finding roms with size {} and MD5 {} and parent id {}",
            size, md5, parent_id
        )
    })
}

#[cfg(feature = "ird")]
pub async fn count_roms_with_romfile_by_size_and_md5_and_parent_id(
    connection: &mut SqliteConnection,
    size: u64,
    md5: &str,
    parent_id: i64,
) -> i32 {
    let size = i64::try_from(size).unwrap();
    let md5 = md5.to_lowercase();
    sqlx::query!(
        "
        SELECT count(id) AS 'count!'
        FROM roms
        WHERE romfile_id IS NOT NULL
        AND size = ?
        AND md5 = ?
        AND parent_id = ?
        ",
        size,
        md5,
        parent_id,
    )
    .fetch_one(connection)
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Error while finding roms with size {} and MD5 {} and parent id {}",
            size, md5, parent_id
        )
    })
    .count
}

pub async fn find_rom_by_name_and_game_id(
    connection: &mut SqliteConnection,
    name: &str,
    game_id: i64,
) -> Option<Rom> {
    sqlx::query_as!(
        Rom,
        "
        SELECT *
        FROM roms
        WHERE name = ?
        AND game_id = ?
        ",
        name,
        game_id,
    )
    .fetch_optional(connection)
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Error while finding rom with name {} and game id {}",
            name, game_id
        )
    })
}

pub async fn find_rom_by_size_and_crc_and_game_id(
    connection: &mut SqliteConnection,
    size: i64,
    crc: &str,
    game_id: i64,
) -> Option<Rom> {
    let crc = crc.to_lowercase();
    sqlx::query_as!(
        Rom,
        "
        SELECT *
        FROM roms
        WHERE size = ?
        AND crc = ?
        AND game_id = ?
        ",
        size,
        crc,
        game_id,
    )
    .fetch_optional(connection)
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Error while finding rom with size {} and CRC {} and game id {}",
            size, crc, game_id
        )
    })
}

pub async fn delete_rom_by_name_and_game_id(
    connection: &mut SqliteConnection,
    name: &str,
    game_id: i64,
) {
    sqlx::query!(
        "
        DELETE FROM roms
        WHERE name = ?
        AND game_id = ?
        ",
        name,
        game_id,
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| {
        panic!(
            "Error while deleting rom with name {} and game_id {}",
            name, game_id
        )
    });
}

pub async fn create_romfile(connection: &mut SqliteConnection, path: &str, size: u64) -> i64 {
    let size = i64::try_from(size).unwrap();
    sqlx::query!(
        "
        INSERT INTO romfiles (path, size)
        VALUES (?, ?)
        ",
        path,
        size,
    )
    .execute(connection)
    .await
    .expect("Error while creating romfile")
    .last_insert_rowid()
}

pub async fn update_romfile(connection: &mut SqliteConnection, id: i64, path: &str, size: u64) {
    let size = i64::try_from(size).unwrap();
    sqlx::query!(
        "
        UPDATE romfiles 
        SET path = ?, size = ?
        WHERE id = ?
        ",
        path,
        size,
        id,
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while updating romfile with id {}", id));
}

pub async fn find_romfiles(connection: &mut SqliteConnection) -> Vec<Romfile> {
    sqlx::query_as!(
        Romfile,
        "
        SELECT *
        FROM romfiles
        ORDER BY path
        ",
    )
    .fetch_all(connection)
    .await
    .expect("Error while finding romfiles")
}

pub async fn find_romfiles_by_ids(connection: &mut SqliteConnection, ids: &[i64]) -> Vec<Romfile> {
    let sql = format!(
        "
    SELECT *
    FROM romfiles
    WHERE id IN ({})
    ",
        ids.iter().join(",")
    );
    sqlx::query_as::<_, Romfile>(&sql)
        .fetch_all(connection)
        .await
        .expect("Error while finding romfiles")
}

pub async fn find_romfiles_by_system_id(
    connection: &mut SqliteConnection,
    system_id: i64,
) -> Vec<Romfile> {
    sqlx::query_as!(
        Romfile,
        "
        SELECT *
        FROM romfiles
        WHERE id IN (
            SELECT DISTINCT(romfile_id)
            FROM roms
            WHERE game_id IN (
                SELECT id
                FROM games
                WHERE system_id = ?
            )
        )
        ",
        system_id,
    )
    .fetch_all(connection)
    .await
    .expect("Error while finding romfiles in trash")
}

pub async fn find_romfiles_in_trash(connection: &mut SqliteConnection) -> Vec<Romfile> {
    sqlx::query_as!(
        Romfile,
        "
        SELECT *
        FROM romfiles
        WHERE path LIKE \"%/Trash/%\"
        ORDER BY path
        ",
    )
    .fetch_all(connection)
    .await
    .expect("Error while finding romfiles in trash")
}

pub async fn find_playlists_by_system_id(
    connection: &mut SqliteConnection,
    system_id: i64,
) -> Vec<Romfile> {
    sqlx::query_as!(
        Romfile,
        "
        SELECT *
        FROM romfiles
        WHERE id IN (
            SELECT DISTINCT(playlist_id)
            FROM games
            WHERE system_id = ?
        )
        ",
        system_id,
    )
    .fetch_all(connection)
    .await
    .expect("Error while finding romfiles in trash")
}

pub async fn find_romfile_by_path(
    connection: &mut SqliteConnection,
    path: &str,
) -> Option<Romfile> {
    sqlx::query_as!(
        Romfile,
        "
        SELECT *
        FROM romfiles
        WHERE path = ?
        ",
        path,
    )
    .fetch_optional(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while finding romfile with path {}", path))
}

pub async fn find_romfile_by_id(connection: &mut SqliteConnection, id: i64) -> Romfile {
    sqlx::query_as!(
        Romfile,
        "
        SELECT *
        FROM romfiles
        WHERE id = ?
        ",
        id,
    )
    .fetch_one(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while finding romfile with id {}", id))
}

pub async fn delete_romfile_by_id(connection: &mut SqliteConnection, id: i64) {
    sqlx::query!(
        "
        DELETE FROM romfiles
        WHERE id = ?
        ",
        id,
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while deleting romfile with id {}", id));
}

pub async fn delete_romfiles_without_rom(connection: &mut SqliteConnection) {
    sqlx::query!(
        "
        DELETE
        FROM romfiles
        WHERE id NOT IN (
            SELECT DISTINCT(romfile_id)
            FROM roms 
            WHERE romfile_id IS NOT NULL
        )
        "
    )
    .execute(connection)
    .await
    .expect("Error while finding romfiles without rom");
}

pub async fn create_header_from_xml(
    connection: &mut SqliteConnection,
    detector_xml: &DetectorXml,
    system_id: i64,
) -> i64 {
    let size = i64::from_str_radix(&detector_xml.rule.start_offset, 16).unwrap();
    sqlx::query!(
        "
        INSERT INTO headers (name, version, size, system_id)
        VALUES (?, ?, ?, ?)
        ",
        detector_xml.name,
        detector_xml.version,
        size,
        system_id,
    )
    .execute(connection)
    .await
    .expect("Error while creating header")
    .last_insert_rowid()
}

pub async fn update_header_from_xml(
    connection: &mut SqliteConnection,
    id: i64,
    detector_xml: &DetectorXml,
    system_id: i64,
) {
    let size = i64::from_str_radix(&detector_xml.rule.start_offset, 16).unwrap();
    sqlx::query!(
        "
        UPDATE headers
        SET name = ?, version = ?, size = ?, system_id = ?
        WHERE id = ?
        ",
        detector_xml.name,
        detector_xml.version,
        size,
        system_id,
        id,
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while updating header with id {}", id));
}

pub async fn find_header_by_system_id(
    connection: &mut SqliteConnection,
    system_id: i64,
) -> Option<Header> {
    sqlx::query_as!(
        Header,
        "
        SELECT *
        FROM headers
        WHERE system_id = ?
        ",
        system_id,
    )
    .fetch_optional(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while finding header with system id {}", system_id))
}

pub async fn create_rule_from_xml(
    connection: &mut SqliteConnection,
    data_xml: &DataXml,
    header_id: i64,
) -> i64 {
    let start_byte = i64::from_str_radix(&data_xml.offset, 16).unwrap();
    sqlx::query!(
        "
        INSERT INTO rules (start_byte, hex_value, header_id)
        VALUES (?, ?, ?)
        ",
        start_byte,
        data_xml.value,
        header_id,
    )
    .execute(connection)
    .await
    .expect("Error while creating rule")
    .last_insert_rowid()
}

pub async fn find_rules_by_header_id(
    connection: &mut SqliteConnection,
    header_id: i64,
) -> Vec<Rule> {
    sqlx::query_as!(
        Rule,
        "
        SELECT *
        FROM rules
        WHERE header_id = ?
        ",
        header_id,
    )
    .fetch_all(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while finding rules with header id {}", header_id))
}

pub async fn delete_rules_by_header_id(connection: &mut SqliteConnection, header_id: i64) {
    sqlx::query!(
        "
        DELETE FROM rules
        WHERE header_id = ?
        ",
        header_id,
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while deleting rules with header_id {}", header_id));
}

pub async fn create_setting(connection: &mut SqliteConnection, key: &str, value: Option<String>) {
    sqlx::query!(
        "
        INSERT INTO settings (key, value)
        VALUES (?, ?)
        ",
        key,
        value,
    )
    .execute(connection)
    .await
    .expect("Error while creating setting");
}

pub async fn update_setting(connection: &mut SqliteConnection, id: i64, value: Option<String>) {
    sqlx::query!(
        "
        UPDATE settings
        SET value = ?
        WHERE id = ?
        ",
        value,
        id,
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while updating setting with id {}", id));
}

pub async fn find_settings(connection: &mut SqliteConnection) -> Vec<Setting> {
    sqlx::query_as!(
        Setting,
        "
        SELECT *
        FROM settings
        ORDER BY key
        ",
    )
    .fetch_all(connection)
    .await
    .expect("Error while finding settings")
}

pub async fn find_setting_by_key(connection: &mut SqliteConnection, key: &str) -> Option<Setting> {
    sqlx::query_as!(
        Setting,
        "
        SELECT *
        FROM settings
        WHERE key = ?
        ",
        key,
    )
    .fetch_optional(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while finding setting with key {}", key))
}
