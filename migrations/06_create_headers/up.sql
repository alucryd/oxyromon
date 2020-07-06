CREATE TABLE headers (
    id INTEGER NOT NULL PRIMARY KEY,
    name VARCHAR NOT NULL,
    version VARCHAR NOT NULL,
    start_byte BIGINT NOT NULL,
    size BIGINT NOT NULL,
    hex_value VARCHAR NOT NULL,
    system_id INTEGER NOT NULL,
    FOREIGN KEY (system_id) REFERENCES systems(id) ON DELETE CASCADE,
    UNIQUE (system_id)
);