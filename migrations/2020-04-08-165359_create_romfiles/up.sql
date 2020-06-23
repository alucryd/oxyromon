CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

CREATE TABLE romfiles (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    path VARCHAR UNIQUE NOT NULL
);

ALTER TABLE roms ADD romfile_id UUID;
ALTER TABLE roms ADD CONSTRAINT roms_romfile_id_fkey FOREIGN KEY (romfile_id) REFERENCES romfiles(id) ON DELETE SET NULL;
