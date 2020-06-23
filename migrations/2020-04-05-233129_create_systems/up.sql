CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

CREATE TABLE systems (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name VARCHAR UNIQUE NOT NULL,
    description VARCHAR NOT NULL,
    version VARCHAR NOT NULL
);