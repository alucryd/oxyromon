ALTER TABLE roms DROP CONSTRAINT roms_romfile_id_fkey;
ALTER TABLE roms DROP romfile_id;

DROP TABLE romfiles;