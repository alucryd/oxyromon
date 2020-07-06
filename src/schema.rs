table! {
    games (id) {
        id -> Integer,
        name -> Text,
        description -> Text,
        regions -> Text,
        system_id -> Integer,
        parent_id -> Nullable<Integer>,
    }
}

table! {
    headers (id) {
        id -> Integer,
        name -> Text,
        version -> Text,
        start_byte -> BigInt,
        size -> BigInt,
        hex_value -> Text,
        system_id -> Integer,
    }
}

table! {
    releases (id) {
        id -> Integer,
        name -> Text,
        region -> Text,
        game_id -> Integer,
    }
}

table! {
    romfiles (id) {
        id -> Integer,
        path -> Text,
    }
}

table! {
    roms (id) {
        id -> Integer,
        name -> Text,
        size -> BigInt,
        crc -> Text,
        md5 -> Text,
        sha1 -> Text,
        rom_status -> Nullable<Text>,
        game_id -> Integer,
        romfile_id -> Nullable<Integer>,
    }
}

table! {
    systems (id) {
        id -> Integer,
        name -> Text,
        description -> Text,
        version -> Text,
    }
}

joinable!(games -> systems (system_id));
joinable!(headers -> systems (system_id));
joinable!(releases -> games (game_id));
joinable!(roms -> games (game_id));
joinable!(roms -> romfiles (romfile_id));

allow_tables_to_appear_in_same_query!(games, headers, releases, romfiles, roms, systems,);
