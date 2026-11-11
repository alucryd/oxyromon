CREATE TABLE IF NOT EXISTS clone_lists (
    id INTEGER NOT NULL PRIMARY KEY,
    name VARCHAR NOT NULL,
    version VARCHAR NOT NULL,
    system_id INTEGER NOT NULL UNIQUE,
    FOREIGN KEY (system_id) REFERENCES systems(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS clone_list_titles (
    id INTEGER NOT NULL PRIMARY KEY,
    group_name VARCHAR NOT NULL,
    search_term VARCHAR NOT NULL,
    name_type INTEGER NOT NULL,
    priority INTEGER NOT NULL,
    match_regions VARCHAR,
    match_languages VARCHAR,
    match_string VARCHAR,
    clone_list_id INTEGER NOT NULL,
    FOREIGN KEY (clone_list_id) REFERENCES clone_lists(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS clone_list_titles_clone_list_id ON clone_list_titles (clone_list_id);
