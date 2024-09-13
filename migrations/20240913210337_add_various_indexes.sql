
CREATE INDEX IF NOT EXISTS systems_name ON systems (name);
CREATE INDEX IF NOT EXISTS games_name ON games (name);
CREATE INDEX IF NOT EXISTS roms_name ON roms (name);
CREATE INDEX IF NOT EXISTS romfiles_path ON romfiles (path);
CREATE INDEX IF NOT EXISTS patches_index ON patches ("index");
CREATE INDEX IF NOT EXISTS patches_rom_id ON patches (rom_id);
