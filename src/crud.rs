use super::model::*;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use rayon::prelude::*;
use std::convert::TryFrom;
use uuid::Uuid;

pub fn create_system(connection: &PgConnection, system_xml: &SystemXml) -> System {
    use schema::systems;
    diesel::insert_into(systems::table)
        .values(&SystemInput::from(system_xml))
        .get_result(connection)
        .expect("Error while creating system")
}

pub fn update_system<'a>(
    connection: &PgConnection,
    system: &System,
    system_xml: &SystemXml,
) -> System {
    diesel::update(system)
        .set(&SystemInput::from(system_xml))
        .get_result(connection)
        .expect(&format!("Error while updating system {}", system.name))
}

pub fn find_system_by_name<'a>(connection: &PgConnection, system_name: &str) -> Option<System> {
    use schema::systems::dsl::*;
    systems
        .filter(name.eq(system_name))
        .get_result(connection)
        .optional()
        .expect(&format!("Error while finding system {}", system_name))
}

pub fn find_systems<'a>(connection: &PgConnection) -> Vec<System> {
    use schema::systems::dsl::*;
    systems
        .get_results(connection)
        .expect(&format!("Error while finding systems"))
}

pub fn create_game<'a>(
    connection: &PgConnection,
    game_xml: &GameXml,
    regions: &String,
    system_uuid: &Uuid,
    parent_uuid: Option<&Uuid>,
) -> Game {
    use schema::games;
    diesel::insert_into(games::table)
        .values(&GameInput::from((
            game_xml,
            regions,
            system_uuid,
            parent_uuid,
        )))
        .get_result(connection)
        .expect("Error while creating game")
}

pub fn update_game<'a>(
    connection: &PgConnection,
    game: &Game,
    game_xml: &GameXml,
    regions: &String,
    system_uuid: &Uuid,
    parent_uuid: Option<&Uuid>,
) -> Game {
    diesel::update(game)
        .set(&GameInput::from((
            game_xml,
            regions,
            system_uuid,
            parent_uuid,
        )))
        .get_result(connection)
        .expect(&format!("Error while updating game {}", game.name))
}

pub fn find_games_by_system<'a>(connection: &PgConnection, system: &System) -> Vec<Game> {
    Game::belonging_to(system)
        .get_results(connection)
        .expect(&format!(
            "Error while finding games for system {}",
            system.id
        ))
}

pub fn find_grouped_games_by_system<'a>(
    connection: &PgConnection,
    system: &System,
) -> Vec<(Game, Vec<Game>)> {
    use schema::games::dsl::*;
    let parent_games = Game::belonging_to(system)
        .filter(parent_id.is_null())
        .get_results(connection)
        .expect(&format!(
            "Error while finding games for system {}",
            system.id
        ));
    let clone_games = Game::belonging_to(&parent_games)
        .get_results(connection)
        .expect(&format!(
            "Error while finding clone games for system {}",
            system.id
        ))
        .grouped_by(&parent_games);
    parent_games.into_par_iter().zip(clone_games).collect()
}

pub fn find_game_names_by_system<'a>(connection: &PgConnection, system: &System) -> Vec<String> {
    use schema::games::dsl::*;
    Game::belonging_to(system)
        .select(name)
        .get_results(connection)
        .expect(&format!(
            "Error while finding games for system {}",
            system.id
        ))
}

pub fn find_game_by_system_and_name<'a>(
    connection: &PgConnection,
    system: &System,
    game_name: &str,
) -> Option<Game> {
    use schema::games::dsl::*;
    Game::belonging_to(system)
        .filter(name.eq(game_name))
        .get_result(connection)
        .optional()
        .expect(&format!(
            "Error while finding game {} for system {}",
            game_name, system.id
        ))
}

pub fn delete_game_by_system_and_name<'a>(
    connection: &PgConnection,
    system: &System,
    game_name: &str,
) {
    use schema::games::dsl::*;
    diesel::delete(Game::belonging_to(system).filter(name.eq(game_name)))
        .execute(connection)
        .expect(&format!(
            "Error while deleting game {} for system {:?}",
            game_name, system.id
        ));
}

pub fn create_release<'a>(
    connection: &PgConnection,
    release_xml: &ReleaseXml,
    game_uuid: &Uuid,
) -> Release {
    use schema::releases;
    diesel::insert_into(releases::table)
        .values(&ReleaseInput::from((release_xml, game_uuid)))
        .get_result(connection)
        .expect("Error while creating release")
}

pub fn update_release<'a>(
    connection: &PgConnection,
    release: &Release,
    release_xml: &ReleaseXml,
    game_uuid: &Uuid,
) -> Release {
    diesel::update(release)
        .set(&ReleaseInput::from((release_xml, game_uuid)))
        .get_result(connection)
        .expect(&format!("Error while updating release {}", release.name))
}

pub fn find_release_by_game_id_and_name_and_region<'a>(
    connection: &PgConnection,
    game_uuid: &Uuid,
    release_name: &str,
    release_region: &str,
) -> Option<Release> {
    use schema::releases::dsl::*;
    releases
        .filter(
            game_id
                .eq(game_uuid)
                .and(name.eq(release_name))
                .and(region.eq(release_region)),
        )
        .get_result(connection)
        .optional()
        .expect(&format!(
            "Error while finding release {} for game {}",
            release_name, game_uuid
        ))
}

pub fn create_rom<'a>(connection: &PgConnection, rom_xml: &RomXml, game_uuid: &Uuid) -> Rom {
    use schema::roms;
    diesel::insert_into(roms::table)
        .values(&RomInput::from((rom_xml, game_uuid)))
        .get_result(connection)
        .expect("Error while creating rom")
}

pub fn update_rom<'a>(
    connection: &PgConnection,
    rom: &Rom,
    rom_xml: &RomXml,
    game_uuid: &Uuid,
) -> Rom {
    diesel::update(rom)
        .set(&RomInput::from((rom_xml, game_uuid)))
        .get_result(connection)
        .expect(&format!("Error while updating rom {}", rom.name))
}

pub fn update_rom_romfile<'a>(connection: &PgConnection, rom: &Rom, romfile_uuid: &Uuid) -> Rom {
    use schema::roms::dsl::*;
    diesel::update(rom)
        .set(romfile_id.eq(romfile_uuid))
        .get_result(connection)
        .expect(&format!(
            "Error while updating rom {} with file {}",
            rom.name, romfile_uuid
        ))
}

pub fn find_roms_by_game_id<'a>(connection: &PgConnection, game_uuid: &Uuid) -> Vec<Rom> {
    use schema::roms::dsl::*;
    roms.filter(game_id.eq(game_uuid))
        .get_results(connection)
        .expect(&format!("Error while finding roms for game {}", game_uuid))
}

pub fn find_roms_romfiles_with_romfile_by_games<'a>(
    connection: &PgConnection,
    games: &Vec<Game>,
) -> Vec<Vec<(Rom, Romfile)>> {
    use schema::romfiles;
    Rom::belonging_to(games)
        .inner_join(romfiles::table)
        .get_results(connection)
        .expect("Error while finding roms and romfiles")
        .grouped_by(games)
}

pub fn find_games_roms_romfiles_with_romfile_by_system<'a>(
    connection: &PgConnection,
    system: &System,
) -> Vec<(Game, Vec<(Rom, Romfile)>)> {
    use schema::romfiles;
    use schema::roms;
    let games = Game::belonging_to(system)
        .get_results(connection)
        .expect("Error while finding games");
    let roms_romfiles = Rom::belonging_to(&games)
        .inner_join(romfiles::table)
        .order_by(roms::name.asc())
        .get_results(connection)
        .expect("Error while finding roms and romfiles")
        .grouped_by(&games);
    games
        .into_par_iter()
        .zip(roms_romfiles)
        .filter(|(_, roms_romfiles)| !roms_romfiles.is_empty())
        .collect()
}

pub fn find_roms_without_romfile_by_games<'a>(
    connection: &PgConnection,
    games: &Vec<Game>,
) -> Vec<Rom> {
    use schema::roms::dsl::*;
    Rom::belonging_to(games)
        .filter(romfile_id.is_null())
        .order_by(name.asc())
        .get_results(connection)
        .expect("Error while finding roms")
}

pub fn find_rom_by_game_id_and_name<'a>(
    connection: &PgConnection,
    game_uuid: &Uuid,
    rom_name: &str,
) -> Option<Rom> {
    use schema::roms::dsl::*;
    roms.filter(game_id.eq(game_uuid).and(name.eq(rom_name)))
        .get_result(connection)
        .optional()
        .expect(&format!(
            "Error while finding rom {} for game {}",
            rom_name, game_uuid
        ))
}

pub fn find_roms_by_size_and_crc_and_system<'a>(
    connection: &PgConnection,
    rom_size: u64,
    rom_crc: &str,
    system_uuid: &Uuid,
) -> Vec<Rom> {
    use schema::games;
    use schema::roms::dsl::*;
    let rom_game: Vec<(Rom, Game)> = roms
        .inner_join(games::table)
        .filter(
            size.eq(&i64::try_from(rom_size).unwrap())
                .and(crc.eq(rom_crc.to_lowercase()))
                .and(games::dsl::system_id.eq(system_uuid)),
        )
        .get_results(connection)
        .expect(&format!(
            "Error while finding rom with size {} and CRC {} for system {}",
            rom_size, rom_crc, system_uuid
        ));
    rom_game
        .into_iter()
        .map(|rom_game_system| rom_game_system.0)
        .collect()
}

pub fn create_romfile<'a>(connection: &PgConnection, romfile_input: &RomfileInput) -> Romfile {
    use schema::romfiles;
    diesel::insert_into(romfiles::table)
        .values(romfile_input)
        .get_result(connection)
        .expect("Error while creating file")
}

pub fn update_romfile<'a>(
    connection: &PgConnection,
    file: &Romfile,
    file_input: &RomfileInput,
) -> Romfile {
    diesel::update(file)
        .set(file_input)
        .get_result(connection)
        .expect(&format!("Error while updating file {}", file.path))
}

pub fn find_romfile_by_id<'a>(connection: &PgConnection, file_uuid: &Uuid) -> Option<Romfile> {
    use schema::romfiles::dsl::*;
    romfiles
        .filter(id.eq(file_uuid))
        .get_result(connection)
        .optional()
        .expect(&format!("Error while finding file {}", file_uuid))
}

pub fn find_romfile_by_path<'a>(connection: &PgConnection, file_path: &str) -> Option<Romfile> {
    use schema::romfiles::dsl::*;
    romfiles
        .filter(path.eq(file_path))
        .get_result(connection)
        .optional()
        .expect(&format!("Error while finding file with path {}", file_path))
}

pub fn find_romfiles_in_trash<'a>(connection: &PgConnection) -> Vec<Romfile> {
    use schema::romfiles::dsl::*;
    romfiles
        .filter(path.like("%/Trash/%"))
        .order_by(path.asc())
        .get_results(connection)
        .expect(&format!("Error while finding romfiles in trash"))
}

pub fn find_romfiles<'a>(connection: &PgConnection) -> Vec<Romfile> {
    use schema::romfiles::dsl::*;
    romfiles
        .get_results(connection)
        .expect(&format!("Error while finding romfiles"))
}

pub fn delete_romfile_by_id<'a>(connection: &PgConnection, romfile_uuid: &Uuid) {
    use schema::romfiles::dsl::*;
    diesel::delete(romfiles.filter(id.eq(romfile_uuid)))
        .execute(connection)
        .expect(&format!(
            "Error while deleting romfile with id {}",
            romfile_uuid
        ));
}

pub fn create_header<'a>(
    connection: &PgConnection,
    detector_xml: &DetectorXml,
    system_uuid: &Uuid,
) -> Header {
    use schema::headers;
    diesel::insert_into(headers::table)
        .values(&HeaderInput::from((detector_xml, system_uuid)))
        .get_result(connection)
        .expect("Error while creating header")
}

pub fn update_header<'a>(
    connection: &PgConnection,
    header: &Header,
    detector_xml: &DetectorXml,
    system_uuid: &Uuid,
) -> Header {
    diesel::update(header)
        .set(&HeaderInput::from((detector_xml, system_uuid)))
        .get_result(connection)
        .expect(&format!("Error while updating header {}", header.name))
}

pub fn find_header_by_system_id<'a>(
    connection: &PgConnection,
    system_uuid: &Uuid,
) -> Option<Header> {
    use schema::headers::dsl::*;
    headers
        .filter(system_id.eq(system_uuid))
        .get_result(connection)
        .optional()
        .expect(&format!(
            "Error while finding header for system {}",
            system_uuid
        ))
}
