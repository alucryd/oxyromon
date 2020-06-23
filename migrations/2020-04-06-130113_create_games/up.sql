CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

CREATE TABLE games (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name VARCHAR NOT NULL,
    description VARCHAR NOT NULL,
    regions VARCHAR NOT NULL,
    system_id UUID NOT NULL,
    parent_id UUID,
    FOREIGN KEY (system_id) REFERENCES systems(id) ON DELETE CASCADE,
    FOREIGN KEY (parent_id) REFERENCES games(id) ON DELETE CASCADE,
    UNIQUE (name, system_id)
);