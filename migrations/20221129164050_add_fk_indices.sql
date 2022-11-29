CREATE INDEX IF NOT EXISTS games_system_id ON games (system_id);
CREATE INDEX IF NOT EXISTS games_parent_id ON games (parent_id);
CREATE INDEX IF NOT EXISTS games_bios_id ON games (bios_id);
CREATE INDEX IF NOT EXISTS roms_game_id ON roms (game_id);
CREATE INDEX IF NOT EXISTS roms_parent_id ON roms (parent_id);
CREATE INDEX IF NOT EXISTS roms_romfile_id ON roms (romfile_id);
CREATE INDEX IF NOT EXISTS headers_system_id ON headers (system_id);
CREATE INDEX IF NOT EXISTS rules_header_id ON rules (header_id);