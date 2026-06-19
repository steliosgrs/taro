-- Versioned-document registry (Slice 1: Config Registry).
-- A `document` is a named handle in an open-enum `namespace`, global/cross-experiment.
-- A `document_version` is an immutable, content-addressed snapshot of that handle:
-- `content_hash` = sha256 of the canonical-JSON body; `UNIQUE(document_id, content_hash)`
-- makes re-publishing identical content idempotent. `version` is a monotonic
-- human label per document. `parent_version_id` is the lineage edge (Slice 2 datasets).
-- `body` is opaque JSON validated for STRUCTURE ONLY — the server never interprets it.

CREATE TABLE document (
    id          TEXT PRIMARY KEY,
    namespace   TEXT NOT NULL,             -- open enum: 'config' | 'dataset' | …
    name        TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    UNIQUE (namespace, name)               -- a handle is unique within its namespace
);

CREATE TABLE document_version (
    id                TEXT PRIMARY KEY,
    document_id       TEXT NOT NULL REFERENCES document(id),
    version           INTEGER NOT NULL,    -- monotonic per document; the human label
    content_hash      TEXT NOT NULL,       -- sha256 of canonical-JSON body
    body              TEXT NOT NULL,       -- opaque JSON document (a config / recipe)
    parent_version_id TEXT REFERENCES document_version(id),  -- lineage edge (nullable)
    created_at        TEXT NOT NULL,
    UNIQUE (document_id, version),
    UNIQUE (document_id, content_hash)     -- per-document dedup: same body = same version
);
CREATE INDEX ix_docver_document ON document_version(document_id);
CREATE INDEX ix_docver_parent   ON document_version(parent_version_id);

CREATE TABLE run_document (                -- which versions a run was launched from
    run_id     TEXT NOT NULL REFERENCES run(id),
    version_id TEXT NOT NULL REFERENCES document_version(id),
    role       TEXT NOT NULL,              -- open enum: 'config' | 'dataset' | …
    PRIMARY KEY (run_id, role, version_id)
);
CREATE INDEX ix_rundoc_version ON run_document(version_id);
