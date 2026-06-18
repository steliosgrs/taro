# CLAUDE.md

Guidance for working in this repo. Taro is a self-hosted, **curve-native**
experiment tracker (an MLflow alternative): the load-bearing idea is that a
metric value can be a **curve/vector**, stored as structured data so N runs' PR
curves can be overlaid — the thing MLflow can't do.

## Layout

| Path | What |
|---|---|
| `server/` | Rust REST API (axum + sqlx + SQLite). The storage/serving core. |
| `clients/python/` | Python SDK (`taro`) + framework adapters. |
| `scripts/` | `validate_yolo_adapter.py` — Ultralytics integration probe. |
| `docs/` | Design (`poc-design.md` is canonical, `architecture.md`, `airflow-integration.md`). |
| `notes/` | Private Obsidian vault — **gitignored**. See `notes/INDEX.md` for the tree. |

## Run the server

```bash
cd server
cargo run                 # 0.0.0.0:8080; creates taro.db and ./taro_blobs
curl localhost:8080/health
```

Config (env vars, all optional — see `server/.env.example`):
`TARO_DATABASE_URL` (`sqlite://taro.db`), `TARO_BIND` (`0.0.0.0:8080`),
`TARO_API_KEY` (unset = auth disabled), `TARO_BLOB_ROOT` (`./taro_blobs`).

Checks: `cargo build`, `cargo clippy`. Migrations in `server/migrations/` run
automatically on startup.

## Python SDK

```bash
cd clients/python
uv venv && uv pip install -e .          # core is stdlib-only, zero deps
# real YOLO adapter / probe also need: uv pip install ultralytics
```

```python
import taro
taro.init("http://localhost:8080")
with taro.start_run("exp", params={"lr0": 0.01}) as run:
    run.log_metric("mAP50", 0.64, step=50)
    run.log_curve("pr_curve", x=recall, y=precision, step=50, curve_type="pr")
    run.log_artifact("weights/best.pt")
overlay = taro.compare_curves([a, b], key="pr_curve")   # data, not a PNG
```

Smoke test: `python examples/validate_m3.py` (needs the server up).

## REST API (`/api/v1`, frozen wire contract — see `docs/poc-design.md §4`)

`POST /experiments` · `POST /runs` · `PATCH /runs/{id}` · `GET /runs/{id}` ·
`POST|GET /runs/{id}/metrics` (scalar) · `POST|GET /runs/{id}/curves` ·
`POST|GET /runs/{id}/artifacts` · `GET /curves/compare?run_ids=A,B&key=&step=latest`.

## Conventions & invariants

- **Server is framework-agnostic** — vocabulary is only
  experiment/run/metric/curve/param/tag/artifact. All ML logic lives in the
  Python adapters (`taro.integrations.*`), never the server.
- Only a **`running`** run accepts metrics/curves/artifacts; finished runs are
  immutable.
- `scalar_metric` and `curve_metric` are **separate tables** (many tiny rows vs
  few fat rows). `curve_type` is an **open enum** the server never rejects.
  Curves validate **structure only** (equal lengths, finite) — never ML correctness.
- Artifacts: DB stores metadata only (`name/uri/media_type/size`); bytes go to the
  `BlobStore` (`LocalFs` now, S3 later).
- SDK is **never-crash**: tracking failures warn and continue; an unreachable
  server yields a degraded no-op `Run` (`run.ok is False`). Training must never die
  because of logging.
- IDs are uuid v7 strings; timestamps RFC3339 strings (server-stamped `ts`,
  client-supplied `step`).

## Status

POC milestones **M0–M6 complete**: wire contract, server skeleton, scalar path,
curve path + `/curves/compare`, Python SDK, artifacts + blob store, Ultralytics
adapter. Data access is behind the `Store` trait (`src/store.rs`, `SqliteStore`
impl) — a `PostgresStore` is the remaining productionization step. CLI and a UI
are post-POC. Airflow orchestration is explicitly **out of scope** (the server
must never orchestrate — see `docs/airflow-integration.md`).

## Workflow notes

- Commit/push only when asked. Milestone commits go on `main` (`M<n>: …`).
- `notes/`, `.obsidian/`, `*.db`, `taro_blobs/`, `target/` are gitignored.
