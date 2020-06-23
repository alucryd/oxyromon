CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

CREATE TABLE headers (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name VARCHAR NOT NULL,
    version VARCHAR NOT NULL,
    start INTEGER NOT NULL,
    size INTEGER NOT NULL,
    hex_value VARCHAR NOT NULL,
    system_id UUID NOT NULL,
    FOREIGN KEY (system_id) REFERENCES systems(id) ON DELETE CASCADE
);
