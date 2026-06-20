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

## Docker (M11)

One-command playground: `docker compose --profile seed up --build` brings up the
server + Postgres and loads two demo runs (the Iris example). Drop `--profile
seed` for an empty server; `docker compose down -v` wipes volumes. The server
image (`server/Dockerfile`) is multi-stage and self-contained — migrations are
embedded into the binary at compile time, so the runtime image ships no SQL and
the build needs no database. Compose wires Postgres; a bare `docker run` of the
image falls back to SQLite under `/data`. Seed image: `clients/python/Dockerfile`.

## Python SDK

```bash
cd clients/python
uv venv && uv pip install -e .          # core is stdlib-only, zero deps
# real YOLO adapter / probe also need: uv pip install ultralytics
```

```python
import taro
taro.init("http://localhost:8080")
cfg = taro.register_config("yolo-baseline", {"lr0": 0.01, "epochs": 100})  # M13; returns a version id
ds  = taro.register_dataset("coco-vehicles",                               # M14; dataset recipe
        base={"manifest_hash": "…", "uri": "s3://data/coco"},
        ops=[{"op": "mosaic", "p": 0.5}])
with taro.start_run("exp", params={"lr0": 0.01}, config_version_id=cfg) as run:
    run.link_document(ds, role="dataset")   # recipes link via the role'd endpoint
    run.log_metric("mAP50", 0.64, step=50)
    run.log_curve("pr_curve", x=recall, y=precision, step=50, curve_type="pr")
    run.log_artifact("weights/best.pt")
overlay = taro.compare_curves([a, b], key="pr_curve")   # data, not a PNG
```

`register_config` is the soft default for the config registry (M13): optional and
never-crash (returns `None` on failure → the run just starts un-linked); identical
content is deduped server-side, so calling it every run is cheap. The config is the
run's structured source of record, separate from free `params`.
`register_dataset` (M14, registry Slice 2) is the same primitive under
`namespace="dataset"`: it publishes a declarative recipe `{base, ops}` and takes a
`parent_version_id` for variation-of-a-variation lineage. The server stores the
recipe as opaque data and **never executes it** (an adapter applies it); recipes
link to a run via `run.link_document(version_id, role="dataset")` (inline
`config_version_id` is config-only by design).

Smoke test: `python examples/validate_m3.py` (needs the server up).

## CLI (M10)

Installing the SDK exposes a `taro` console command (also `python -m taro`) — a
thin read/inspect client over the REST API. Logging stays in training code (the
SDK); the CLI is for looking at what's there. Config via `--url` (env `TARO_URL`,
default `http://localhost:8080`) and `--api-key` (env `TARO_API_KEY`); `--json`
emits the raw response. Source: `clients/python/taro/cli.py`.

```bash
taro health
taro experiments list                       # also: experiments create <name> | get <id>
taro runs list [--experiment ID] [--status S] [--limit N]   # newest-first (M12)
taro runs get <id>                          # detail incl. params + tags
taro runs diff <run_a> <run_b>              # params/tags/latest-metric diff (M12)
taro runs metrics <id> [--key K]            # scalar series
taro runs curves <id> [--key K] [--step S]  # curve metrics
taro runs artifacts <id>
taro runs documents <id>                    # configs linked to a run (M13)
taro compare A,B --key pr_curve             # the overlay, as a table
taro documents list [--namespace N] [--name X]   # config registry (M13)
taro documents get <id>                     # handle + version history
taro documents create <namespace> <name>    # also: publish <id> <body.json> [--parent V]
taro versions get <id>                      # version detail incl. body
taro versions runs <id>                     # reverse: runs launched from a version
```

## REST API (`/api/v1`, frozen wire contract — see `docs/poc-design.md §4`)

`POST /experiments` · `POST /runs` ·
`GET /runs?experiment_id=&status=&limit=` (list, newest-first; M12) ·
`PATCH /runs/{id}` · `GET /runs/{id}` ·
`POST|GET /runs/{id}/metrics` (scalar) · `POST|GET /runs/{id}/curves` ·
`POST|GET /runs/{id}/artifacts` · `GET /curves/compare?run_ids=A,B&key=&step=latest`.

Config registry (M13): `POST|GET /documents` (`?namespace=&name=`) ·
`GET /documents/{id}` (+ versions) · `POST /documents/{id}/versions` (publish,
content-addressed/deduped) · `GET /versions/{id}` · `GET /versions/{id}/runs`
(reverse lookup) · `POST|GET /runs/{id}/documents` (link/list). A run may also be
linked inline via `config_version_id` on `POST /runs`.

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
- **Document registry** (M13 configs / M14 dataset recipes): a `document` is a
  named handle in an open-enum `namespace` (`config`, `dataset`); a `document_version`
  is an **immutable, content-addressed** snapshot (sha256 of the canonical-JSON
  `body`; re-publishing identical content is deduped, not re-versioned). The `body`
  is **opaque JSON validated for structure only** (must be an object) — the server
  never interprets it. `run_document` links a version to a run under a `role`,
  giving provenance both ways. Configs **coexist with** `param`: params stay the
  flattened queryable index; the linked version is the structured source.
- SDK is **never-crash**: tracking failures warn and continue; an unreachable
  server yields a degraded no-op `Run` (`run.ok is False`). Training must never die
  because of logging.
- IDs are uuid v7 strings; timestamps RFC3339 strings (server-stamped `ts`,
  client-supplied `step`).

## Status

POC milestones **M0–M14 complete**: wire contract, server skeleton, scalar path,
curve path + `/curves/compare`, Python SDK, artifacts + blob store, Ultralytics
adapter, integration test suite (M7), `PostgresStore` engine parity (M8),
**streaming artifact upload** (M9 — request body flows to the `BlobStore` chunk
by chunk; the SDK streams the file handle, never reading it whole), the
**`taro` CLI** (M10 — read/inspect client in the SDK; see the CLI section above),
**Docker packaging** (M11 — compose stack + demo seed; see the Docker section), and
the **run-listing endpoint + CLI character features** (M12 — `GET /runs` with
experiment/status/limit filters; CLI `runs list` and `runs diff`), and the
**config registry** (M13 — Slice 1 of the versioned-document registry epic:
`document`/`document_version`/`run_document`, content-addressed publish, inline +
endpoint run linking, SDK `register_config`, CLI `documents`/`versions`), and
**dataset recipes** (M14 — Slice 2 of that epic: the same primitive under
`namespace="dataset"`, a declarative `{base, ops}` recipe body with
`parent_version_id` lineage, linked via `role="dataset"`; SDK `register_dataset`;
no new server code — the server stores the recipe and never executes it).
Data access is behind the `Store` trait (`src/store.rs`);
both `SqliteStore` and `PostgresStore` are proven at parity. A UI is post-POC. Airflow orchestration is
explicitly **out of scope** (the server must never orchestrate — see
`docs/airflow-integration.md`).

## Workflow notes

- Commit/push only when asked. Milestone commits go on `main` (`M<n>: …`).
- `notes/`, `.obsidian/`, `*.db`, `taro_blobs/`, `target/` are gitignored.
