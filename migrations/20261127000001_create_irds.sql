-- A PS3 game's IRD as it was imported, which rebuilds its ISO from its JB
-- folder and extracts the ISO back to it
CREATE TABLE IF NOT EXISTS irds (
    id INTEGER NOT NULL PRIMARY KEY,
    game_id INTEGER NOT NULL UNIQUE,
    data BLOB NOT NULL,
    FOREIGN KEY (game_id) REFERENCES games(id) ON DELETE CASCADE
);
