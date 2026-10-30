-- Metadados do próprio serviço (chave/valor). Guarda o verificador do TOKEN_PEPPER.
CREATE TABLE schema_meta (
    key   TEXT PRIMARY KEY,
    value BLOB NOT NULL
) STRICT;
