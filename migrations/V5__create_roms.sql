CREATE TABLE IF NOT EXISTS roms (
    id INTEGER NOT NULL PRIMARY KEY,
    name VARCHAR NOT NULL,
    size BIGINT NOT NULL,
    crc VARCHAR(8) NOT NULL,
    md5 VARCHAR(32) NOT NULL,
    sha1 VARCHAR(40) NOT NULL,
    rom_status VARCHAR,
    game_id INTEGER NOT NULL,
    romfile_id INTEGER,
    FOREIGN KEY (game_id) REFERENCES games(id) ON DELETE CASCADE,
    FOREIGN KEY (romfile_id) REFERENCES romfiles(id) ON DELETE SET NULL,
    UNIQUE (name, game_id)
);

CREATE INDEX IF NOT EXISTS roms_size_crc ON roms (size, crc);
