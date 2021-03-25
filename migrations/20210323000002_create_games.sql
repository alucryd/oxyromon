CREATE TABLE IF NOT EXISTS games (
    id INTEGER NOT NULL PRIMARY KEY,
    name VARCHAR NOT NULL,
    description VARCHAR NOT NULL,
    regions VARCHAR NOT NULL,
    system_id INTEGER NOT NULL,
    parent_id INTEGER,
    FOREIGN KEY (system_id) REFERENCES systems(id) ON DELETE CASCADE,
    FOREIGN KEY (parent_id) REFERENCES games(id) ON DELETE CASCADE,
    UNIQUE (name, system_id)
);
