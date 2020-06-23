CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

CREATE TABLE releases (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name VARCHAR NOT NULL,
    region VARCHAR NOT NULL,
    game_id UUID NOT NULL,
    FOREIGN KEY (game_id) REFERENCES games(id) ON DELETE CASCADE,
    UNIQUE (name, region, game_id)
);