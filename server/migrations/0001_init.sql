-- Taro POC schema (SQLite dialect).
-- Full 7-entity schema so later milestones (metrics, curves, artifacts) need no
-- migration churn; M1 only wires experiment/run/param/tag in code.
-- IDs and timestamps are TEXT (uuid v7 string / RFC3339) for a frictionless POC;
-- a Postgres migration with native UUID/TIMESTAMPTZ is a later refinement.

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
    id     INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL REFERENCES run(id),
    key    TEXT NOT NULL,
    step   INTEGER NOT NULL,
    value  REAL NOT NULL,
    ts     TEXT NOT NULL
);
CREATE INDEX ix_scalar_run_key_step ON scalar_metric(run_id, key, step);

CREATE TABLE curve_metric (                -- M3 (the differentiator)
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id     TEXT NOT NULL REFERENCES run(id),
    key        TEXT NOT NULL,
    step       INTEGER NOT NULL,
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
    size_bytes INTEGER,
    created_at TEXT NOT NULL
);
