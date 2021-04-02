use super::model::*;
use rayon::prelude::*;
use sqlx::migrate::Migrator;
use sqlx::prelude::*;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};
use sqlx::SqliteConnection;
use std::convert::TryFrom;
use std::str::FromStr;

static MIGRATOR: Migrator = sqlx::migrate!();

pub async fn establish_connection(url: &str) -> SqliteConnection {
    let mut connection: SqliteConnection = SqliteConnectOptions::from_str(url)
        .unwrap()
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .connect()
        .await
        .expect(&format!("Error connecting to {}", url));

    connection
        .execute(
            "
            PRAGMA locking_mode = EXCLUSIVE;
            PRAGMA synchronous = NORMAL;
            PRAGMA temp_store = MEMORY;
            PRAGMA wal_checkpoint(TRUNCATE);
            ",
        )
        .await
        .expect("Failed to setup the database");

    MIGRATOR
        .run(&mut connection)
        .await
        .expect("Failed to run database migrations");

    connection
}

pub async fn close_connection(connection: &mut SqliteConnection) {
    connection
        .execute("PRAGMA optimize")
        .await
        .expect("Failed to optimize the database");
}

pub async fn create_system(connection: &mut SqliteConnection, system_xml: &SystemXml) -> i64 {
    sqlx::query!(
        "
        INSERT INTO systems (name, description, version)
        VALUES (?, ?, ?)
        ",
        system_xml.name,
        system_xml.description,
        system_xml.version,
    )
    .execute(connection)
    .await
    .expect("Error while creating system")
    .last_insert_rowid()
}

pub async fn update_system(connection: &mut SqliteConnection, id: i64, system_xml: &SystemXml) {
    sqlx::query!(
        "
        UPDATE systems
        SET name = ?, description = ?, version = ?
        WHERE id = ?
        ",
        system_xml.name,
        system_xml.description,
        system_xml.version,
        id,
    )
    .execute(connection)
    .await
    .expect(&format!("Error while updating system with id {}", id));
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
    .expect(&format!("Error while finding system with id {}", id))
}

pub async fn find_system_by_name(connection: &mut SqliteConnection, name: &str) -> Option<System> {
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
    .expect(&format!("Error while finding system with name {}", name))
}

pub async fn create_game<'a>(
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
    .expect(&format!("Error while updating game with id {}", id));
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
        ",
        system_id,
    )
    .fetch_all(connection)
    .await
    .expect(&format!(
        "Error while finding games with system id {}",
        system_id
    ))
}

pub async fn find_games_by_ids<'a>(connection: &mut SqliteConnection, ids: &[i64]) -> Vec<Game> {
    let sql = format!(
        "
        SELECT *
        FROM games
        WHERE id IN ({})
        ",
        ids.par_iter()
            .map(ToString::to_string)
            .collect::<Vec<String>>()
            .join(", ")
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
        ",
        system_id,
    )
    .fetch_all(connection)
    .await
    .expect(&format!(
        "Error while finding parent games with system id {}",
        system_id
    ))
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
        ",
        system_id,
    )
    .fetch_all(connection)
    .await
    .expect(&format!(
        "Error while finding parent games with system id {}",
        system_id
    ))
}

pub async fn find_game_by_name_and_system_id(
    connection: &mut SqliteConnection,
    name: &str,
    system_id: i64,
) -> Option<Game> {
    sqlx::query_as!(
        Game,
        "
        SELECT *
        FROM games
        WHERE name = ?
        AND system_id = ?
        ",
        name,
        system_id,
    )
    .fetch_optional(connection)
    .await
    .expect(&format!(
        "Error while finding games with name {} and system id {}",
        name, system_id
    ))
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
    .expect(&format!(
        "Error while deleting game with name {} and system_id {}",
        name, system_id
    ));
}

pub async fn create_rom<'a>(
    connection: &mut SqliteConnection,
    rom_xml: &RomXml,
    game_id: i64,
) -> i64 {
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
    .expect(&format!("Error while updating rom with id {}", id));
}

pub async fn update_rom_romfile(connection: &mut SqliteConnection, id: i64, romfile_id: i64) {
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
    .expect(&format!("Error while updating rom with id {}", id));
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

pub async fn find_roms_by_game_id<'a>(connection: &mut SqliteConnection, game_id: i64) -> Vec<Rom> {
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
    .expect(&format!("Error while finding rom with game id {}", game_id))
}

pub async fn find_roms_without_romfile_by_system_id(
    connection: &mut SqliteConnection,
    system_id: i64,
) -> Vec<Rom> {
    sqlx::query_as!(
        Rom,
        "
        SELECT *
        FROM roms
        WHERE romfile_id IS NULL
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
    .expect("Error while finding roms without romfile")
}

pub async fn find_roms_without_romfile_by_game_ids<'a>(
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
        game_ids
            .par_iter()
            .map(ToString::to_string)
            .collect::<Vec<String>>()
            .join(", ")
    );
    sqlx::query_as::<_, Rom>(&sql)
        .fetch_all(connection)
        .await
        .expect("Error while finding roms with romfile")
}

pub async fn find_roms_with_romfile_by_system_id<'a>(
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

pub async fn find_roms_with_romfile_by_system_id_and_name<'a>(
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

pub async fn find_roms_with_romfile_by_game_ids<'a>(
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
        game_ids
            .par_iter()
            .map(ToString::to_string)
            .collect::<Vec<String>>()
            .join(", ")
    );
    sqlx::query_as::<_, Rom>(&sql)
        .fetch_all(connection)
        .await
        .expect("Error while finding roms with romfile")
}

pub async fn find_rom_by_name_and_game_id<'a>(
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
    .expect(&format!(
        "Error while finding rom with name {} and game id {}",
        name, game_id
    ))
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
    .expect(&format!(
        "Error while finding rom with size {} and CRC {} and system id {}",
        size, crc, system_id
    ))
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
    .expect(&format!(
        "Error while deleting rom with name {} and game_id {}",
        name, game_id
    ));
}

pub async fn create_romfile(connection: &mut SqliteConnection, path: &str) -> i64 {
    sqlx::query!(
        "
        INSERT INTO romfiles (path)
        VALUES (?)
        ",
        path,
    )
    .execute(connection)
    .await
    .expect(&format!("Error while creating romfile"))
    .last_insert_rowid()
}

pub async fn update_romfile(connection: &mut SqliteConnection, id: i64, path: &str) {
    sqlx::query!(
        "
        UPDATE romfiles 
        SET path = ?
        WHERE id = ?
        ",
        path,
        id,
    )
    .execute(connection)
    .await
    .expect(&format!("Error while updating romfile with id {}", id));
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
    .expect(&format!("Error while finding romfiles"))
}

pub async fn find_romfiles_by_ids<'a>(
    connection: &mut SqliteConnection,
    ids: &[i64],
) -> Vec<Romfile> {
    let sql = format!(
        "
    SELECT *
    FROM romfiles
    WHERE id IN ({})
    ",
        ids.par_iter()
            .map(ToString::to_string)
            .collect::<Vec<String>>()
            .join(", ")
    );
    sqlx::query_as::<_, Romfile>(&sql)
        .fetch_all(connection)
        .await
        .expect(&format!("Error while finding romfiles"))
}

pub async fn find_romfiles_by_system_id<'a>(
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
    .expect(&format!("Error while finding romfiles in trash"))
}

pub async fn find_romfiles_in_trash<'a>(connection: &mut SqliteConnection) -> Vec<Romfile> {
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
    .expect(&format!("Error while finding romfiles in trash"))
}

pub async fn find_romfile_by_path<'a>(
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
    .expect(&format!("Error while finding romfile with path {}", path))
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
    .expect(&format!("Error while finding romfile with id {}", id))
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
    .expect(&format!("Error while deleting setting with id {}", id));
}

pub async fn delete_romfiles_without_rom<'a>(connection: &mut SqliteConnection) {
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
    .expect(&format!("Error while finding romfiles without rom"));
}

pub async fn create_header(
    connection: &mut SqliteConnection,
    detector_xml: &DetectorXml,
    system_id: i64,
) -> i64 {
    let start_byte = i64::from_str_radix(&detector_xml.rule.start_offset, 16).unwrap();
    let size = i64::from_str_radix(&detector_xml.rule.data.offset, 16).unwrap();
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
    .expect(&format!("Error while creating header"))
    .last_insert_rowid()
}

pub async fn update_header(
    connection: &mut SqliteConnection,
    id: i64,
    detector_xml: &DetectorXml,
    system_id: i64,
) {
    let start_byte = i64::from_str_radix(&detector_xml.rule.start_offset, 16).unwrap();
    let size = i64::from_str_radix(&detector_xml.rule.data.offset, 16).unwrap();
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
    .expect(&format!("Error while updating header with id {}", id));
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
    .expect(&format!(
        "Error while finding header with system id {}",
        system_id
    ))
}

pub async fn create_setting(connection: &mut SqliteConnection, key: &str, value: &str) {
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
    .expect(&format!("Error while creating setting"));
}

pub async fn update_setting(connection: &mut SqliteConnection, id: i64, value: &str) {
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
    .expect(&format!("Error while updating setting with id {}", id));
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
    .expect(&format!("Error while finding setting with key {}", key))
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
    .expect(&format!("Error while deleting setting with key {}", key));
}
