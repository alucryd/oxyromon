table! {
    games (id) {
        id -> Uuid,
        name -> Varchar,
        description -> Varchar,
        regions -> Varchar,
        system_id -> Uuid,
        parent_id -> Nullable<Uuid>,
    }
}

table! {
    headers (id) {
        id -> Uuid,
        name -> Varchar,
        version -> Varchar,
        start -> Int4,
        size -> Int4,
        hex_value -> Varchar,
        system_id -> Uuid,
    }
}

table! {
    releases (id) {
        id -> Uuid,
        name -> Varchar,
        region -> Varchar,
        game_id -> Uuid,
    }
}

table! {
    romfiles (id) {
        id -> Uuid,
        path -> Varchar,
    }
}

table! {
    roms (id) {
        id -> Uuid,
        name -> Varchar,
        size -> Int8,
        crc -> Varchar,
        md5 -> Varchar,
        sha1 -> Varchar,
        status -> Nullable<Varchar>,
        game_id -> Uuid,
        romfile_id -> Nullable<Uuid>,
    }
}

table! {
    systems (id) {
        id -> Uuid,
        name -> Varchar,
        description -> Varchar,
        version -> Varchar,
    }
}

joinable!(games -> systems (system_id));
joinable!(headers -> systems (system_id));
joinable!(releases -> games (game_id));
joinable!(roms -> games (game_id));
joinable!(roms -> romfiles (romfile_id));

allow_tables_to_appear_in_same_query!(
    games,
    headers,
    releases,
    romfiles,
    roms,
    systems,
);
