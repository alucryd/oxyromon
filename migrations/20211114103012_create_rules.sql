CREATE TABLE IF NOT EXISTS rules (
    id INTEGER NOT NULL PRIMARY KEY,
    start_byte BIGINT NOT NULL,
    size BIGINT NOT NULL,
    hex_value VARCHAR NOT NULL,
    header_id INTEGER NOT NULL,
    FOREIGN KEY (header_id) REFERENCES headers(id) ON DELETE CASCADE
);

INSERT INTO rules (start_byte, size, hex_value, header_id)
SELECT start_byte, size, hex_value, id
FROM headers;

ALTER TABLE headers DROP COLUMN start_byte;
ALTER TABLE headers DROP COLUMN size;
ALTER TABLE headers DROP COLUMN hex_value;
ALTER TABLE headers ADD COLUMN operation VARCHAR;
