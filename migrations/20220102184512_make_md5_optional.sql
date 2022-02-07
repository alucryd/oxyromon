ALTER TABLE roms
RENAME COLUMN md5 TO md5_old;

ALTER TABLE roms
ADD COLUMN md5 VARCHAR(32);

UPDATE roms
SET md5 = md5_old;

ALTER TABLE roms
DROP COLUMN md5_old;