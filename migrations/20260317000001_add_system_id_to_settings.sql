CREATE TABLE settings_new (
    id INTEGER NOT NULL PRIMARY KEY,
    key VARCHAR NOT NULL,
    value VARCHAR,
    system_id INTEGER REFERENCES systems(id) ON DELETE CASCADE
);

INSERT INTO settings_new (id, key, value)
SELECT id, key, value FROM settings;

DROP TABLE settings;

ALTER TABLE settings_new RENAME TO settings;

CREATE UNIQUE INDEX settings_key_global_unique ON settings(key) WHERE system_id IS NULL;
CREATE UNIQUE INDEX settings_key_system_unique ON settings(key, system_id) WHERE system_id IS NOT NULL;
