CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

CREATE TABLE roms (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name VARCHAR NOT NULL,
    size BIGINT NOT NULL,
    crc VARCHAR(8) NOT NULL,
    md5 VARCHAR(32) NOT NULL,
    sha1 VARCHAR(40) NOT NULL,
    STATUS VARCHAR,
    game_id UUID NOT NULL,
    FOREIGN KEY (game_id) REFERENCES games(id) ON DELETE CASCADE,
    UNIQUE (name, game_id)
);

CREATE INDEX roms_size_crc ON roms (size, crc);
