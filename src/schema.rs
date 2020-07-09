table! {
    games (id) {
        id -> BigInt,
        name -> Text,
        description -> Text,
        regions -> Text,
        system_id -> BigInt,
        parent_id -> Nullable<BigInt>,
    }
}

table! {
    headers (id) {
        id -> BigInt,
        name -> Text,
        version -> Text,
        start_byte -> BigInt,
        size -> BigInt,
        hex_value -> Text,
        system_id -> BigInt,
    }
}

table! {
    releases (id) {
        id -> BigInt,
        name -> Text,
        region -> Text,
        game_id -> BigInt,
    }
}

table! {
    romfiles (id) {
        id -> BigInt,
        path -> Text,
    }
}

table! {
    roms (id) {
        id -> BigInt,
        name -> Text,
        size -> BigInt,
        crc -> Text,
        md5 -> Text,
        sha1 -> Text,
        rom_status -> Nullable<Text>,
        game_id -> BigInt,
        romfile_id -> Nullable<BigInt>,
    }
}

table! {
    settings (id) {
        id -> BigInt,
        key -> Text,
        value -> Nullable<Text>,
    }
}

table! {
    systems (id) {
        id -> BigInt,
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
