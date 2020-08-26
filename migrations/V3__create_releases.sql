CREATE TABLE IF NOT EXISTS releases (
    id INTEGER NOT NULL PRIMARY KEY,
    name VARCHAR NOT NULL,
    region VARCHAR NOT NULL,
    game_id INTEGER NOT NULL,
    FOREIGN KEY (game_id) REFERENCES games(id) ON DELETE CASCADE,
    UNIQUE (name, region, game_id)
);
