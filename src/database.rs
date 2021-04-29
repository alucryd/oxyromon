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
        .connect_timeout(Duration::from_secs(5))
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

pub async fn begin_transaction<'a>(
    connection: &'a mut SqliteConnection,
) -> Transaction<'a, Sqlite> {
    Acquire::begin(connection)
        .await
        .expect("Failed to begin transaction")
}

pub async fn commit_transaction<'a>(transaction: Transaction<'a, Sqlite>) {
    transaction
        .commit()
        .await
        .expect("Failed to commit transaction");
}

pub async fn rollback_transaction<'a>(transaction: Transaction<'a, Sqlite>) {
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

pub async fn create_system(connection: &mut SqliteConnection, system_xml: &SystemXml) -> i64 {
    let name = system_xml.name.replace(" (Parent-Clone)", "");
    sqlx::query!(
        "
        INSERT INTO systems (name, description, version, url)
        VALUES (?, ?, ?, ?)
        ",
        name,
        system_xml.description,
        system_xml.version,
        system_xml.url,
    )
    .execute(connection)
    .await
    .expect("Error while creating system")
    .last_insert_rowid()
}

pub async fn update_system(connection: &mut SqliteConnection, id: i64, system_xml: &SystemXml) {
    let name = system_xml.name.replace(" (Parent-Clone)", "");
    sqlx::query!(
        "
        UPDATE systems
        SET name = ?, description = ?, version = ?, url = ?
        WHERE id = ?
        ",
        name,
        system_xml.description,
        system_xml.version,
        system_xml.url,
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
    .expect("Error while finding system names")
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

pub async fn find_system_by_name_like(
    connection: &mut SqliteConnection,
    name: &str,
) -> Option<System> {
    sqlx::query_as!(
        System,
        "
        SELECT *
        FROM systems
        WHERE name LIKE ?
        ",
        name,
    )
    .fetch_optional(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while finding system with name {}", name))
}

pub async fn create_game(
    connection: &mut SqliteConnection,
    game_xml: &GameXml,
    regions: &str,
    system_id: i64,
    parent_id: Option<i64>,
) -> i64 {
    sqlx::query!(
        "
        INSERT INTO games (name, description, regions, system_id, parent_id)
        VALUES (?, ?, ?, ?, ?)
        ",
        game_xml.name,
        game_xml.description,
        regions,
        system_id,
        parent_id,
    )
    .execute(connection)
    .await
    .expect("Error while creating game")
    .last_insert_rowid()
}

pub async fn update_game(
    connection: &mut SqliteConnection,
    id: i64,
    game_xml: &GameXml,
    regions: &str,
    system_id: i64,
    parent_id: Option<i64>,
) {
    sqlx::query!(
        "
        UPDATE games
        SET name = ?, description = ?, regions = ?, system_id = ?, parent_id = ?
        WHERE id = ?
        ",
        game_xml.name,
        game_xml.description,
        regions,
        system_id,
        parent_id,
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
        AND NOT EXISTS (
            SELECT r.id
            FROM roms r
            WHERE r.game_id = games.id
            AND r.romfile_id IS null
        )
        ",
        system_id,
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while marking games as complete"));
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
        AND EXISTS (
            SELECT r.id
            FROM roms r
            WHERE r.game_id = games.id
            AND r.romfile_id IS null
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
        AND EXISTS (
            SELECT r.id
            FROM roms r
            WHERE r.game_id = games.id
            AND r.romfile_id IS null
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
    let sql = format!(
        "
        UPDATE games
        SET sorting = {}
        WHERE id IN ({})
        ",
        sorting as i8,
        ids.iter().join(",")
    );
    sqlx::query(&sql)
        .execute(connection)
        .await
        .unwrap_or_else(|_| panic!("Error while updating games sorting"))
        .rows_affected()
}

pub async fn find_games(connection: &mut SqliteConnection) -> Vec<Game> {
    sqlx::query_as!(
        Game,
        "
        SELECT id, name, description, regions, sorting as \"sorting: _\", complete, system_id, parent_id
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
        SELECT id, name, description, regions, sorting as \"sorting: _\", complete, system_id, parent_id
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
        SELECT id, name, description, regions, sorting as \"sorting: _\", complete, system_id, parent_id
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
        SELECT id, name, description, regions, sorting as \"sorting: _\", complete, system_id, parent_id
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
        SELECT id, name, description, regions, sorting as \"sorting: _\", complete, system_id, parent_id
        FROM games
        WHERE id = ?
        ",
        id,
    )
    .fetch_one(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while finding game with id {}", id))
}

pub async fn find_game_by_name_and_system_id(
    connection: &mut SqliteConnection,
    name: &str,
    system_id: i64,
) -> Option<Game> {
    sqlx::query_as!(
        Game,
        "
        SELECT id, name, description, regions, sorting as \"sorting: _\", complete, system_id, parent_id
        FROM games
        WHERE name = ?
        AND system_id = ?
        ",
        name,
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

pub async fn create_rom(connection: &mut SqliteConnection, rom_xml: &RomXml, game_id: i64) -> i64 {
    let crc = rom_xml.crc.to_lowercase();
    let md5 = rom_xml.md5.to_lowercase();
    let sha1 = rom_xml.sha1.to_lowercase();
    sqlx::query!(
        "
        INSERT INTO roms (name, size, crc, md5, sha1, rom_status, game_id)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        ",
        rom_xml.name,
        rom_xml.size,
        crc,
        md5,
        sha1,
        rom_xml.status,
        game_id,
    )
    .execute(connection)
    .await
    .expect("Error while creating rom")
    .last_insert_rowid()
}

pub async fn update_rom(
    connection: &mut SqliteConnection,
    id: i64,
    rom_xml: &RomXml,
    game_id: i64,
) {
    let crc = rom_xml.crc.to_lowercase();
    let md5 = rom_xml.md5.to_lowercase();
    let sha1 = rom_xml.sha1.to_lowercase();
    sqlx::query!(
        "
        UPDATE roms
        SET name = ?, size = ?, crc = ?, md5 = ?, sha1 = ?, rom_status = ?, game_id = ?
        WHERE id = ?
        ",
        rom_xml.name,
        rom_xml.size,
        crc,
        md5,
        sha1,
        rom_xml.status,
        game_id,
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

pub async fn find_roms_by_game_id(connection: &mut SqliteConnection, game_id: i64) -> Vec<Rom> {
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
    .unwrap_or_else(|_| panic!("Error while finding rom with game id {}", game_id))
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
        SELECT *
        FROM roms
        WHERE romfile_id IS NOT NULL
        AND game_id IN (
            SELECT id
            FROM games
            WHERE system_id = ?
        )
        ORDER BY name
        ",
        system_id,
    )
    .fetch_all(connection)
    .await
    .expect("Error while finding roms with romfile")
}

pub async fn find_roms_with_romfile_by_system_id_and_name(
    connection: &mut SqliteConnection,
    system_id: i64,
    name: &str,
) -> Vec<Rom> {
    let sql = format!(
        "
        SELECT *
        FROM roms
        WHERE romfile_id IS NOT NULL
        AND game_id IN (
            SELECT id
            FROM games
            WHERE system_id = {}
            AND name LIKE '%{}%'
        )
        ORDER BY name
    ",
        system_id, name
    );
    sqlx::query_as::<_, Rom>(&sql)
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

pub async fn find_roms_by_size_and_crc_and_system_id(
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
        SELECT r.id, r.name, r.size, r.crc, r.md5, r.sha1, r.rom_status, r.game_id, r.romfile_id
        FROM roms AS r
        JOIN games AS g ON r.game_id = g.id
        WHERE r.size = ?
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
            "Error while finding rom with size {} and CRC {} and system id {}",
            size, crc, system_id
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
            SELECT romfile_id
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
    .unwrap_or_else(|_| panic!("Error while deleting setting with id {}", id));
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

pub async fn create_header(
    connection: &mut SqliteConnection,
    detector_xml: &DetectorXml,
    system_id: i64,
) -> i64 {
    let start_byte = i64::from_str_radix(&detector_xml.rule.data.offset, 16).unwrap();
    let size = i64::from_str_radix(&detector_xml.rule.start_offset, 16).unwrap();
    sqlx::query!(
        "
        INSERT INTO headers (name, version, start_byte, size, hex_value, system_id)
        VALUES (?, ?, ?, ?, ?, ?)
        ",
        detector_xml.name,
        detector_xml.version,
        start_byte,
        size,
        detector_xml.rule.data.value,
        system_id,
    )
    .execute(connection)
    .await
    .expect("Error while creating header")
    .last_insert_rowid()
}

pub async fn update_header(
    connection: &mut SqliteConnection,
    id: i64,
    detector_xml: &DetectorXml,
    system_id: i64,
) {
    let start_byte = i64::from_str_radix(&detector_xml.rule.data.offset, 16).unwrap();
    let size = i64::from_str_radix(&detector_xml.rule.start_offset, 16).unwrap();
    sqlx::query!(
        "
        UPDATE headers
        SET name = ?, version = ?, start_byte = ?, size = ?, hex_value = ?, system_id = ?
        WHERE id = ?
        ",
        detector_xml.name,
        detector_xml.version,
        start_byte,
        size,
        detector_xml.rule.data.value,
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

pub async fn delete_setting_by_key(connection: &mut SqliteConnection, key: &str) {
    sqlx::query!(
        "
        DELETE FROM settings
        WHERE key = ?
        ",
        key,
    )
    .execute(connection)
    .await
    .unwrap_or_else(|_| panic!("Error while deleting setting with key {}", key));
}
