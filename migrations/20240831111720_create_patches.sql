CREATE TABLE IF NOT EXISTS patches (
    id INTEGER NOT NULL PRIMARY KEY,
    name VARCHAR NOT NULL,
    "index" INTEGER NOT NULL,
    rom_id INTEGER NOT NULL,
    romfile_id INTEGER NOT NULL,
    FOREIGN KEY (rom_id) REFERENCES roms(id) ON DELETE CASCADE,
    FOREIGN KEY (romfile_id) REFERENCES romfiles(id) ON DELETE CASCADE,
    UNIQUE (name, rom_id)
);

