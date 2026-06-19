-- Versioned-document registry (Slice 1: Config Registry) — Postgres dialect,
-- a structural mirror of migrations/0002_documents.sql. IDs/timestamps/body stay
-- TEXT so the shared `FromRow` models decode from either backend; only `version`
-- takes a native numeric type (BIGINT). See the SQLite copy for the design notes.

CREATE TABLE document (
    id          TEXT PRIMARY KEY,
    namespace   TEXT NOT NULL,
    name        TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    UNIQUE (namespace, name)
);

CREATE TABLE document_version (
    id                TEXT PRIMARY KEY,
    document_id       TEXT NOT NULL REFERENCES document(id),
    version           BIGINT NOT NULL,
    content_hash      TEXT NOT NULL,
    body              TEXT NOT NULL,
    parent_version_id TEXT REFERENCES document_version(id),
    created_at        TEXT NOT NULL,
    UNIQUE (document_id, version),
    UNIQUE (document_id, content_hash)
);
CREATE INDEX ix_docver_document ON document_version(document_id);
CREATE INDEX ix_docver_parent   ON document_version(parent_version_id);

CREATE TABLE run_document (
    run_id     TEXT NOT NULL REFERENCES run(id),
    version_id TEXT NOT NULL REFERENCES document_version(id),
    role       TEXT NOT NULL,
    PRIMARY KEY (run_id, role, version_id)
);
CREATE INDEX ix_rundoc_version ON run_document(version_id);
