-- Taro POC schema (Postgres dialect) — mirror of migrations/0001_init.sql.
-- Kept structurally identical so the M7 engine-generic suite proves parity:
-- IDs/timestamps stay TEXT (uuid v7 string / RFC3339) and curve `data` stays
-- TEXT (JSON), so the same `FromRow` models decode from either backend. Only
-- the numeric/auto-increment columns take native PG types (BIGSERIAL/BIGINT/
-- DOUBLE PRECISION). Native UUID/TIMESTAMPTZ/JSONB are a deliberate later
-- refinement — they would change how columns decode and break the shared models.

CREATE TABLE experiment (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    created_at  TEXT NOT NULL
);

CREATE TABLE run (
    id            TEXT PRIMARY KEY,
    experiment_id TEXT NOT NULL REFERENCES experiment(id),
    name          TEXT,
    status        TEXT NOT NULL,          -- running | finished | failed | killed
    started_at    TEXT NOT NULL,
    ended_at      TEXT
);
CREATE INDEX ix_run_experiment ON run(experiment_id);

CREATE TABLE param (                       -- immutable
    run_id TEXT NOT NULL REFERENCES run(id),
    key    TEXT NOT NULL,
    value  TEXT NOT NULL,
    PRIMARY KEY (run_id, key)
);

CREATE TABLE tag (                         -- mutable (upsert)
    run_id TEXT NOT NULL REFERENCES run(id),
    key    TEXT NOT NULL,
    value  TEXT NOT NULL,
    PRIMARY KEY (run_id, key)
);

CREATE TABLE scalar_metric (               -- M2
    id     BIGSERIAL PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES run(id),
    key    TEXT NOT NULL,
    step   BIGINT NOT NULL,
    value  DOUBLE PRECISION NOT NULL,
    ts     TEXT NOT NULL
);
CREATE INDEX ix_scalar_run_key_step ON scalar_metric(run_id, key, step);

CREATE TABLE curve_metric (                -- M3 (the differentiator)
    id         BIGSERIAL PRIMARY KEY,
    run_id     TEXT NOT NULL REFERENCES run(id),
    key        TEXT NOT NULL,
    step       BIGINT NOT NULL,
    curve_type TEXT NOT NULL,              -- pr | roc | per_class | generic_xy
    x_label    TEXT,
    y_label    TEXT,
    data       TEXT NOT NULL,              -- JSON (typed axes + parallel arrays)
    ts         TEXT NOT NULL
);
CREATE INDEX ix_curve_run_key_step ON curve_metric(run_id, key, step);

CREATE TABLE artifact (                    -- M5
    id         TEXT PRIMARY KEY,
    run_id     TEXT NOT NULL REFERENCES run(id),
    name       TEXT NOT NULL,
    uri        TEXT NOT NULL,
    media_type TEXT,
    size_bytes BIGINT,
    created_at TEXT NOT NULL
);
