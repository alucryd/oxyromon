use super::model::*;
use super::schema::*;
use diesel::prelude::*;
use diesel::SqliteConnection;
use rayon::prelude::*;
use std::convert::TryFrom;

pub fn create_system(connection: &SqliteConnection, system_xml: &SystemXml) -> System {
    let system_input = SystemInput::from(system_xml);
    diesel::insert_into(systems::table)
        .values(&system_input)
        .execute(connection)
        .expect("Error while creating system");
    find_system_by_name(connection, &system_input.name).unwrap()
}

pub fn update_system<'a>(
    connection: &SqliteConnection,
    system: &System,
    system_xml: &SystemXml,
) -> System {
    let system_input = SystemInput::from(system_xml);
    diesel::update(system)
        .set(&system_input)
        .execute(connection)
        .expect(&format!("Error while updating system with name {}", system.name));
    find_system_by_name(connection, &system_input.name).unwrap()
}

pub fn find_system_by_name<'a>(connection: &SqliteConnection, system_name: &str) -> Option<System> {
    systems::table
        .filter(systems::dsl::name.eq(system_name))
        .get_result(connection)
        .optional()
        .expect(&format!("Error while finding system with name {}", system_name))
}

pub fn find_systems<'a>(connection: &SqliteConnection) -> Vec<System> {
    systems::table
        .get_results(connection)
        .expect(&format!("Error while finding systems"))
}

pub fn create_game<'a>(
    connection: &SqliteConnection,
    game_xml: &GameXml,
    regions: &String,
    system_id: i64,
    parent_id: Option<i64>,
) -> Game {
    let game_input = GameInput::from((game_xml, regions, system_id, parent_id));
    diesel::insert_into(games::table)
        .values(&game_input)
        .execute(connection)
        .expect("Error while creating game");
    find_game_by_name_and_system_id(connection, &game_input.name, system_id).unwrap()
}

pub fn update_game<'a>(
    connection: &SqliteConnection,
    game: &Game,
    game_xml: &GameXml,
    regions: &String,
    system_id: i64,
    parent_id: Option<i64>,
) -> Game {
    let game_input = GameInput::from((game_xml, regions, system_id, parent_id));
    diesel::update(game)
        .set(&game_input)
        .execute(connection)
        .expect(&format!("Error while updating game with name {}", game.name));
    find_game_by_name_and_system_id(connection, &game_input.name, system_id).unwrap()
}

pub fn find_game_by_name_and_system_id<'a>(
    connection: &SqliteConnection,
    name: &str,
    system_id: i64,
) -> Option<Game> {
    games::table
        .filter(games::dsl::name.eq(name))
        .filter(games::dsl::system_id.eq(system_id))
        .get_result(connection)
        .optional()
        .expect(&format!(
            "Error while finding game with name {} for system with id {}",
            name, system_id
        ))
}

pub fn find_games_by_system<'a>(connection: &SqliteConnection, system: &System) -> Vec<Game> {
    Game::belonging_to(system)
        .get_results(connection)
        .expect(&format!(
            "Error while finding games for system with name {}",
            system.name
        ))
}

pub fn find_grouped_games_by_system<'a>(
    connection: &SqliteConnection,
    system: &System,
) -> Vec<(Game, Vec<Game>)> {
    let parent_games = Game::belonging_to(system)
        .filter(games::dsl::parent_id.is_null())
        .get_results(connection)
        .expect(&format!(
            "Error while finding games for system with name {}",
            system.name
        ));
    let clone_games = Game::belonging_to(&parent_games)
        .get_results(connection)
        .expect(&format!(
            "Error while finding clone games for system with name {}",
            system.name
        ))
        .grouped_by(&parent_games);
    parent_games.into_par_iter().zip(clone_games).collect()
}

pub fn find_game_names_by_system<'a>(
    connection: &SqliteConnection,
    system: &System,
) -> Vec<String> {
    Game::belonging_to(system)
        .select(games::dsl::name)
        .get_results(connection)
        .expect(&format!(
            "Error while finding games for system with name {}",
            system.name
        ))
}

pub fn delete_game_by_name_and_system_id<'a>(
    connection: &SqliteConnection,
    name: &str,
    system_id: i64,
) {
    diesel::delete(
        games::table
            .filter(games::dsl::name.eq(name))
            .filter(games::dsl::system_id.eq(system_id)),
    )
    .execute(connection)
    .expect(&format!(
        "Error while deleting game {} for system with id {}",
        name, system_id
    ));
}

pub fn create_release<'a>(
    connection: &SqliteConnection,
    release_xml: &ReleaseXml,
    game_id: i64,
) -> Release {
    let release_input = ReleaseInput::from((release_xml, game_id));
    diesel::insert_into(releases::table)
        .values(&release_input)
        .execute(connection)
        .expect("Error while creating release");
    find_release_by_name_and_region_and_game_id(
        connection,
        &release_input.name,
        &release_input.region,
        game_id,
    )
    .unwrap()
}

pub fn update_release<'a>(
    connection: &SqliteConnection,
    release: &Release,
    release_xml: &ReleaseXml,
    game_id: i64,
) -> Release {
    let release_input = ReleaseInput::from((release_xml, game_id));
    diesel::update(release)
        .set(&release_input)
        .execute(connection)
        .expect(&format!("Error while updating release with name {}", release.name));
    find_release_by_name_and_region_and_game_id(
        connection,
        &release_input.name,
        &release_input.region,
        game_id,
    )
    .unwrap()
}

pub fn find_release_by_name_and_region_and_game_id<'a>(
    connection: &SqliteConnection,
    name: &str,
    region: &str,
    game_id: i64,
) -> Option<Release> {
    releases::table
        .filter(releases::dsl::name.eq(name))
        .filter(releases::dsl::region.eq(region))
        .filter(releases::dsl::game_id.eq(game_id))
        .get_result(connection)
        .optional()
        .expect(&format!(
            "Error while finding release {} for region {} and game with id {}",
            name, region, game_id
        ))
}

pub fn create_rom<'a>(connection: &SqliteConnection, rom_xml: &RomXml, game_id: i64) -> Rom {
    let rom_input = RomInput::from((rom_xml, game_id));
    diesel::insert_into(roms::table)
        .values(&rom_input)
        .execute(connection)
        .expect("Error while creating rom");
    find_rom_by_name_and_game_id(connection, &rom_input.name, game_id).unwrap()
}

pub fn update_rom<'a>(
    connection: &SqliteConnection,
    rom: &Rom,
    rom_xml: &RomXml,
    game_id: i64,
) -> Rom {
    let rom_input = RomInput::from((rom_xml, game_id));
    diesel::update(rom)
        .set(&rom_input)
        .execute(connection)
        .expect(&format!("Error while updating rom with name {}", rom.name));
    find_rom_by_name_and_game_id(connection, &rom_input.name, game_id).unwrap()
}

pub fn update_rom_romfile<'a>(connection: &SqliteConnection, rom: &Rom, romfile_id: i64) -> usize {
    diesel::update(rom)
        .set(roms::dsl::romfile_id.eq(romfile_id))
        .execute(connection)
        .expect(&format!(
            "Error while updating rom with name {} with romfile id {}",
            rom.name, romfile_id
        ))
}

pub fn find_rom_by_name_and_game_id<'a>(
    connection: &SqliteConnection,
    name: &str,
    game_id: i64,
) -> Option<Rom> {
    roms::table
        .filter(roms::dsl::name.eq(name))
        .filter(roms::dsl::game_id.eq(game_id))
        .get_result(connection)
        .optional()
        .expect(&format!(
            "Error while finding rom with {} for game with id {}",
            name, game_id
        ))
}

pub fn find_roms_by_game_id<'a>(connection: &SqliteConnection, game_id: i64) -> Vec<Rom> {
    roms::table
        .filter(roms::dsl::game_id.eq(game_id))
        .get_results(connection)
        .expect(&format!("Error while finding roms for game with id {}", game_id))
}

pub fn find_roms_romfiles_with_romfile_by_games<'a>(
    connection: &SqliteConnection,
    games: &Vec<Game>,
) -> Vec<Vec<(Rom, Romfile)>> {
    Rom::belonging_to(games)
        .inner_join(romfiles::table)
        .get_results(connection)
        .expect("Error while finding roms and romfiles")
        .grouped_by(games)
}

pub fn find_roms_without_romfile_by_games<'a>(
    connection: &SqliteConnection,
    games: &Vec<Game>,
) -> Vec<Rom> {
    use schema::roms::dsl::*;
    Rom::belonging_to(games)
        .filter(romfile_id.is_null())
        .order_by(name.asc())
        .get_results(connection)
        .expect("Error while finding roms")
}

pub fn find_games_roms_romfiles_with_romfile_by_system<'a>(
    connection: &SqliteConnection,
    system: &System,
) -> Vec<(Game, Vec<(Rom, Romfile)>)> {
    let games = Game::belonging_to(system)
        .get_results(connection)
        .expect("Error while finding games");
    let roms_romfiles = Rom::belonging_to(&games)
        .inner_join(romfiles::table)
        .order_by(roms::dsl::name.asc())
        .get_results(connection)
        .expect("Error while finding roms and romfiles")
        .grouped_by(&games);
    games
        .into_par_iter()
        .zip(roms_romfiles)
        .filter(|(_, roms_romfiles)| !roms_romfiles.is_empty())
        .collect()
}

pub fn find_roms_by_size_and_crc_and_system<'a>(
    connection: &SqliteConnection,
    size: u64,
    crc: &str,
    system_id: i64,
) -> Vec<Rom> {
    let roms_games: Vec<(Rom, Game)> = roms::table
        .inner_join(games::table)
        .filter(roms::dsl::size.eq(&i64::try_from(size).unwrap()))
        .filter(roms::dsl::crc.eq(crc.to_lowercase()))
        .filter(games::dsl::system_id.eq(system_id))
        .get_results(connection)
        .expect(&format!(
            "Error while finding rom with size {} and CRC {} for system with id {}",
            size, crc, system_id
        ));
    roms_games.into_iter().map(|rom_game| rom_game.0).collect()
}

pub fn create_romfile<'a>(connection: &SqliteConnection, romfile_input: &RomfileInput) -> Romfile {
    diesel::insert_into(romfiles::table)
        .values(romfile_input)
        .execute(connection)
        .expect("Error while creating romfile");
    find_romfile_by_path(connection, &romfile_input.path).unwrap()
}

pub fn update_romfile<'a>(
    connection: &SqliteConnection,
    romfile: &Romfile,
    romfile_input: &RomfileInput,
) -> Romfile {
    diesel::update(romfile)
        .set(romfile_input)
        .execute(connection)
        .expect(&format!(
            "Error while updating romfile with path {}",
            romfile.path
        ));
    find_romfile_by_path(connection, &romfile_input.path).unwrap()
}

pub fn find_romfile_by_path<'a>(connection: &SqliteConnection, path: &str) -> Option<Romfile> {
    romfiles::table
        .filter(romfiles::dsl::path.eq(path))
        .get_result(connection)
        .optional()
        .expect(&format!("Error while finding file with path {}", path))
}

pub fn find_romfile_by_id<'a>(connection: &SqliteConnection, romfile_id: i64) -> Option<Romfile> {
    romfiles::table
        .filter(romfiles::dsl::id.eq(romfile_id))
        .get_result(connection)
        .optional()
        .expect(&format!(
            "Error while finding romfile with id {}",
            romfile_id
        ))
}

pub fn find_romfiles_in_trash<'a>(connection: &SqliteConnection) -> Vec<Romfile> {
    romfiles::table
        .filter(romfiles::dsl::path.like("%/Trash/%"))
        .order_by(romfiles::dsl::path.asc())
        .get_results(connection)
        .expect(&format!("Error while finding romfiles in trash"))
}

pub fn find_romfiles<'a>(connection: &SqliteConnection) -> Vec<Romfile> {
    romfiles::table
        .get_results(connection)
        .expect(&format!("Error while finding romfiles"))
}

pub fn delete_romfile_by_id<'a>(connection: &SqliteConnection, romfile_id: i64) {
    diesel::delete(romfiles::table.filter(romfiles::dsl::id.eq(romfile_id)))
        .execute(connection)
        .expect(&format!(
            "Error while deleting romfile with id {}",
            romfile_id
        ));
}

pub fn create_header<'a>(
    connection: &SqliteConnection,
    detector_xml: &DetectorXml,
    system_id: i64,
) -> Header {
    let header_input = HeaderInput::from((detector_xml, system_id));
    diesel::insert_into(headers::table)
        .values(&header_input)
        .execute(connection)
        .expect("Error while creating header");
    find_header_by_system_id(connection, system_id).unwrap()
}

pub fn update_header<'a>(
    connection: &SqliteConnection,
    header: &Header,
    detector_xml: &DetectorXml,
    system_id: i64,
) -> Header {
    let header_input = HeaderInput::from((detector_xml, system_id));
    diesel::update(header)
        .set(&header_input)
        .execute(connection)
        .expect(&format!(
            "Error while updating header with name {}",
            header.name
        ));
    find_header_by_system_id(connection, system_id).unwrap()
}

pub fn find_header_by_system_id<'a>(
    connection: &SqliteConnection,
    system_id: i64,
) -> Option<Header> {
    headers::table
        .filter(headers::dsl::system_id.eq(system_id))
        .get_result(connection)
        .optional()
        .expect(&format!(
            "Error while finding header for system {}",
            system_id
        ))
}
