<img src="./docs/images/taro_logo.png" width="35%" height="35%" style="display: block; margin-left: auto; margin-right: auto;"/>

# Taro

**A curve-native experiment tracker for ML.** Self-hosted, fast, and built to store
the metrics MLflow can't compare — precision-recall curves, per-class AP, and other
vector/curve metrics — as _data_, not frozen PNGs.

> **Status: POC complete (M0–M10).** Rust server, scalar + curve metrics with
> `/curves/compare`, the Python SDK, artifacts + blob store, the Ultralytics/YOLO
> adapter, Postgres engine parity, streaming uploads, and the `taro` CLI are all in.
> A web UI is next. See [docs/poc-design.md](docs/poc-design.md).

## Why

MLflow models a metric as a scalar time series. Anything richer — a PR curve, per-class
AP — gets flattened into an image artifact that you can't overlay or compare across runs.
Taro's core idea: **a metric value can be a curve/vector**, stored as structured data, so N
runs' curves can be fetched and compared directly.

## Architecture

- **Server** (Rust: axum + sqlx) — framework-agnostic REST API + storage. Knows only
  `experiment / run / metric / curve / param / tag / artifact`.
- **Python SDK + adapters** (planned) — thin logging client with per-framework shims
  (Ultralytics YOLO, PyTorch, XGBoost).
- **CLI** (planned) — inspect and compare runs without a UI.

See [docs/architecture.md](docs/architecture.md) for diagrams.

## Repository layout

```
server/              Rust server (curve-native tracking backend) + Dockerfile
clients/python/      Python SDK + CLI + framework adapters (+ seed Dockerfile)
docs/                design + architecture documentation
scripts/             dev tooling (e.g. YOLO adapter validation probe)
docker-compose.yml   one-command playground (server + Postgres + demo seed)
```

## Quick start (Docker)

The fastest way to play with Taro — server + Postgres + two demo runs with
overlayable PR curves, in one command:

```bash
docker compose --profile seed up --build   # server on :8080, loads the Iris demo
curl localhost:8080/health
curl "localhost:8080/api/v1/experiments"
docker compose down -v                      # stop and wipe volumes
```

Drop `--profile seed` to start an empty server. The seed container runs once and
exits; the server keeps running. Blobs and Postgres data persist in named volumes.

## Quick start (server, from source)

```bash
cd server
cargo run            # listens on 0.0.0.0:8080, creates ./taro.db (SQLite)
curl localhost:8080/health
```

See [server/README.md](server/README.md) for endpoints and configuration.

## Documentation

- [POC design spec](docs/poc-design.md) — data model, wire contract, milestones
- [Architecture](docs/architecture.md) — system diagrams + CLI design
- [Airflow integration](docs/airflow-integration.md) — future / not in POC

## License

Licensed under the [Apache License 2.0](LICENSE).
