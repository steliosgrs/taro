# Taro — POC Design

Canonical spec for the Taro POC: a self-hosted, **curve-native** experiment tracker. Rust storage + REST API + Python SDK + CLI. **No UI, no orchestration in the POC.**

Companion docs:
- `architecture.md` — system/data/sequence mermaid diagrams + CLI design
- `airflow-integration.md` — FUTURE, deferred (orchestration)

---

## 1. Thesis

MLflow models a metric as a scalar time series `(key, step, value)`. Anything richer — PR curves, per-class AP — gets flattened to opaque PNG artifacts that are **not comparable or overlayable** across runs.

Taro's one load-bearing decision: **a metric value can be a curve/vector, not just a scalar.** Curves are stored as structured data, so N runs' PR curves can be fetched and overlaid. Everything else exists to support that while keeping the server framework-agnostic.

---

## 2. Components

| Component | Responsibility | Boundary |
|---|---|---|
| **API layer (axum)** | HTTP routing, request validation, JSON (de)serialization | Vocabulary is strictly `experiment/run/metric/curve/param/tag/artifact`. No ML concepts. |
| **Domain layer** | Run lifecycle; route metrics to scalar vs curve; invariants (only `running` runs accept metrics) | Pure logic over repository traits. |
| **Persistence (sqlx)** | CRUD + relational integrity; owns schema/migrations | Repository traits → DB engine swappable. |
| **Blob adapter** | Persist artifact bytes, return URI | `BlobStore` trait: `LocalFs` (POC) → `S3` (later). |
| **Python SDK** | Ergonomic logging client (`Run`, `log_metric`, `log_curve`, `log_artifact`); batching, retries | Pure HTTP. No framework imports in core. |
| **Framework adapters** | Map framework events → SDK calls | `taro.integrations.*`; depend only on SDK. |
| **CLI** | Operator interface (no UI): inspect/compare/export | Thin HTTP client over the same REST API. |

All ML-specific logic lives in the Python adapters; the Rust server never knows about YOLO/torch.

---

## 3. Data Model

Entities: `experiment`, `run`, `param`, `tag`, `scalar_metric`, `curve_metric`, `artifact`. See `architecture.md` for the ER diagram.

**Key decision:** `scalar_metric` and `curve_metric` are **separate tables** — many tiny rows (series) vs few fat rows (whole curve per step); different access patterns and indexing. Not a polymorphic column.

### Schema sketch (Postgres dialect)

```sql
CREATE TABLE experiment (
  id UUID PRIMARY KEY, name TEXT UNIQUE NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE run (
  id UUID PRIMARY KEY,
  experiment_id UUID NOT NULL REFERENCES experiment(id),
  name TEXT,
  status TEXT NOT NULL,                  -- running | finished | failed | killed
  started_at TIMESTAMPTZ NOT NULL, ended_at TIMESTAMPTZ
);
CREATE TABLE param (                     -- immutable
  run_id UUID NOT NULL REFERENCES run(id), key TEXT NOT NULL, value TEXT NOT NULL,
  PRIMARY KEY (run_id, key)
);
CREATE TABLE tag (                       -- mutable (upsert)
  run_id UUID NOT NULL REFERENCES run(id), key TEXT NOT NULL, value TEXT NOT NULL,
  PRIMARY KEY (run_id, key)
);
CREATE TABLE scalar_metric (
  id BIGSERIAL PRIMARY KEY,
  run_id UUID NOT NULL REFERENCES run(id),
  key TEXT NOT NULL, step BIGINT NOT NULL, value DOUBLE PRECISION NOT NULL,
  ts TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX ix_scalar_run_key_step ON scalar_metric (run_id, key, step);

CREATE TABLE curve_metric (              -- the differentiator
  id BIGSERIAL PRIMARY KEY,
  run_id UUID NOT NULL REFERENCES run(id),
  key TEXT NOT NULL,                     -- "pr_curve", "per_class_ap"
  step BIGINT NOT NULL,                  -- epoch the curve was computed at
  curve_type TEXT NOT NULL,             -- pr | roc | per_class | generic_xy  (open enum)
  x_label TEXT, y_label TEXT,
  data JSONB NOT NULL,                   -- typed axes + parallel arrays (see §4.4)
  ts TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX ix_curve_run_key_step ON curve_metric (run_id, key, step);

CREATE TABLE artifact (
  id UUID PRIMARY KEY,
  run_id UUID NOT NULL REFERENCES run(id),
  name TEXT NOT NULL, uri TEXT NOT NULL, -- file:///… or s3://…
  media_type TEXT, size_bytes BIGINT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

> **Confusion matrix: out of POC scope.** When added it becomes its own `curve_type="confusion"` with a 2D `matrix` + `labels` payload — it does not fit the x/y-array shape, so it is deferred rather than forced in.

---

## 4. Wire Contract (REST `/api/v1`)

This contract is the thing everything keys off — **freeze it first (M0).**

### 4.1 Endpoints

| Method | Path | Purpose |
|---|---|---|
| POST | `/experiments` | Create / get-or-create |
| POST | `/runs` | Start a run |
| PATCH | `/runs/{id}` | Finalize / update status |
| POST | `/runs/{id}/params` | Bulk params (immutable) |
| POST | `/runs/{id}/tags` | Upsert tags |
| POST | `/runs/{id}/metrics` | Bulk **scalar** metrics |
| POST | `/runs/{id}/curves` | Log **curve** metric(s) |
| POST | `/runs/{id}/artifacts` | Upload (streamed raw body w/ `?name=`, or multipart) or register URI |
| GET | `/runs/{id}` | Run detail |
| GET | `/runs/{id}/metrics?key=` | Scalar series |
| GET | `/runs/{id}/curves?key=&step=` | Curve(s) |
| GET | `/curves/compare?run_ids=a,b&key=&step=` | **Overlay** — N runs' curves for one key |

### 4.2 Start a run
```jsonc
// POST /runs
{ "experiment": "yolo-vehicle-detector", "name": "yolov8n-aug-v3",
  "params": { "model": "yolov8n", "lr0": 0.01, "batch": 16 },
  "tags":   { "git_sha": "a1b2c3", "dataset": "vehicles-v4" } }
// 201 -> { "run_id": "…", "experiment_id": "…", "status": "running" }
```

### 4.3 Log scalar metrics (batched)
```jsonc
// POST /runs/{id}/metrics
{ "metrics": [
  { "key": "train/box_loss", "step": 50, "value": 0.812 },
  { "key": "metrics/mAP50",  "step": 50, "value": 0.643 } ] }
// 202 -> { "accepted": 2 }
```

### 4.4 Log a curve metric (the differentiator)
Payload = **typed axes + parallel arrays**: `data.x` and `data.y` equal-length; optional `series` for multi-line (per-class); optional `labels` for categorical x.
```jsonc
// POST /runs/{id}/curves
{ "curves": [
  { "key": "pr_curve", "step": 50, "curve_type": "pr",
    "x_label": "recall", "y_label": "precision",
    "data": { "x": [0.0, 0.1, ..., 1.0], "y": [1.0, 0.98, ..., 0.40] } },

  { "key": "per_class_ap", "step": 50, "curve_type": "per_class",
    "x_label": "class", "y_label": "AP",
    "data": { "labels": ["car","truck","bus","bike"],
              "x": [0,1,2,3], "y": [0.71,0.66,0.58,0.44] } },

  { "key": "pr_curve_per_class", "step": 50, "curve_type": "pr",
    "x_label": "recall", "y_label": "precision",
    "data": { "x": [0.0, 0.1, ..., 1.0],
              "series": [ { "name": "car",   "y": [1.0, 0.97, ...] },
                          { "name": "truck", "y": [1.0, 0.93, ...] } ] } } ] }
// 202 -> { "accepted": 3 }
```
Rules baked in:
- `curve_type` is an **open enum** (`pr | roc | per_class | generic_xy`); unknown types are stored, never rejected. `generic_xy` is the escape hatch.
- Single shared `x` per record (one or many `y`/`series`) → trivial overlay, compact storage.
- Server validates **structure only** (equal lengths, non-empty) — never ML correctness.

### 4.5 Compare / overlay (the reason Taro exists)
```jsonc
// GET /curves/compare?run_ids=A,B&key=pr_curve&step=latest
{ "key": "pr_curve", "x_label": "recall", "y_label": "precision",
  "runs": [
    { "run_id": "A", "run_name": "v3", "step": 50, "data": { "x": [...], "y": [...] } },
    { "run_id": "B", "run_name": "v2", "step": 50, "data": { "x": [...], "y": [...] } } ] }
```
Returns comparable curve **data, never an image.**

---

## 5. Python SDK (interfaces only)

```python
import taro
taro.init("http://localhost:8080")
with taro.start_run("yolo-vehicle-detector", params={"lr0": 0.01}) as run:
    run.log_metric("mAP50", 0.64, step=50)
    run.log_curve("pr_curve", x=recall, y=precision,
                  step=50, curve_type="pr", x_label="recall", y_label="precision")
    run.log_artifact("runs/train/weights/best.pt")
```
- `Run` is a context manager → auto-finalizes (`finished`/`failed`).
- Internals: background batch queue flushes scalars on size/time; curves + artifacts flush immediately.
- **Never-crash-training policy:** logging failures warn and continue; tracking must never kill a job.
- Adapters (`taro.integrations.{ultralytics,torch,xgboost}`) only call core SDK methods — all ML-specific extraction lives there. YOLO adapter de-risked via `scripts/validate_yolo_adapter.py`.

---

## 6. CLI

Thin HTTP client, ships in the Python package. Headline commands: `taro run ls`, `taro run show`, `taro curve compare --runs A,B --key pr_curve -o pr.json`, `taro curve export`. The `compare`/`export` commands are how curves are overlaid without a UI (export → plot in a notebook). Full design in `architecture.md`.

---

## 7. Tech Stack

| Concern | Choice | Note |
|---|---|---|
| HTTP | **axum** | tokio-native, tower middleware |
| DB access | **sqlx** | compile-time-checked queries, migrations |
| DB engine | **Postgres** (POC may start on **SQLite**) | Postgres for concurrent ingest; SQLite = zero-ops solo start, same code path |
| Serialization | **serde / serde_json** | JSONB curve payloads map directly |
| IDs | **uuid v7** | time-sortable |
| Blob | `std::fs` now; **`object_store`** later | one trait over FS/S3/GCS |
| Errors | thiserror + anyhow | |

**Postgres vs DuckDB:** Postgres for transactional, concurrent *writes* (many runs logging at once). DuckDB is OLAP/columnar — great for heavy cross-run *reads* later as an additive read-side layer, weak for concurrent ingest. Keep repository traits clean so DuckDB/Parquet analytics can be added without touching ingest.

---

## 8. Storage Strategy

- **Curve data → JSONB in the DB** (POC). Curves are small (hundreds of points), infrequent (per-epoch); the overlay query fetches whole rows. Validate equal-length arrays at ingest.
- **Future scale path:** if curves grow large or analytics dominate, dump arrays to **Parquet** in the blob store, keep metadata + URI in DB (mirrors artifacts). The parallel-array JSON shape is forward-compatible.
- **Artifacts → blob store.** Layout `{blob_root}/{experiment_id}/{run_id}/{name}`. DB stores **only** `uri + name + media_type + size`; bytes never enter the DB.
- **Decision rule:** structured numeric data the UI must overlay → DB (`curve_metric`); opaque bytes → blob (`artifact`). A PNG of a curve is an artifact; the curve numbers are a `curve_metric`.

---

## 9. Build Order (Milestones)

| M | Milestone | Outcome |
|---|---|---|
| **M0** | **Freeze wire contract** (§4) | Everything depends on this |
| **M1** | Server skeleton: axum + sqlx + migrations; `experiment`/`run` create/finalize; health | SQLite first |
| **M2** | Scalar path end-to-end (`POST/GET /metrics`) | MLflow baseline |
| **M3** | **Curve path** + `GET /curves/compare` | The milestone that justifies the project |
| **M4** | Python SDK core (`Run`, `log_metric`, `log_curve`, batching, never-crash) | Validate M3 with a fake PR curve |
| **M5** | Artifacts + blob store (`BlobStore`, `LocalFs`, upload) | Weights logged by URI |
| **M6** | Ultralytics adapter (`attach`, `on_fit_epoch_end`/`on_train_end`) | Real YOLO run logging comparable curves |

**POC success criterion:** train two YOLO runs, then `GET /curves/compare` returns both PR curves as overlay-ready data — the exact thing MLflow cannot do.

---

## 10. Risks & Open Decisions (POC)

- **Ultralytics curve extraction** is the #1 integration risk — attribute paths vary by version. Run `scripts/validate_yolo_adapter.py` in the training env **before** building the adapter.
- **DB for day 1:** SQLite (zero-ops solo) vs Postgres (target). Recommend SQLite-via-sqlx with Postgres-compatible SQL.
- **Auth:** static bearer-token stub in one middleware, even for POC.
- **Step semantics:** is `step` always epoch, or epoch + global-iteration? Pick one canonical `step`.
- **Artifact upload:** proxy-through-server, **streamed** (request body → `BlobStore` chunk by chunk; size counted server-side). Large checkpoints never buffer whole in the SDK or server. Presigned URLs deferred.
- **JSONB curve math** (interpolation to a common x-grid for overlay): return raw in POC, align client-side; server-side interpolation is future work.
- **Out of POC scope:** UI/frontend, Airflow integration, confusion-matrix type, model serving.
