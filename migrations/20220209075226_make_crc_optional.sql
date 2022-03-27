DROP INDEX roms_size_crc;

ALTER TABLE roms
RENAME COLUMN crc TO crc_old;

ALTER TABLE roms
ADD COLUMN crc VARCHAR(8);

UPDATE roms
SET crc = crc_old;

ALTER TABLE roms
DROP COLUMN crc_old;

CREATE INDEX IF NOT EXISTS roms_size_crc ON roms (size, crc);
