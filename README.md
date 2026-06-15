<img src="./docs/images/taro_logo.png" width="35%" height="35%" style="display: block; margin-left: auto; margin-right: auto;"/>

# Taro

**A curve-native experiment tracker for ML.** Self-hosted, fast, and built to store
the metrics MLflow can't compare — precision-recall curves, per-class AP, and other
vector/curve metrics — as _data_, not frozen PNGs.

> **Status: early POC / work in progress.** The Rust server skeleton (M1) is running.
> Scalar metrics (M2), curve metrics + comparison (M3), the Python SDK (M4), artifacts
> (M5), and the Ultralytics/YOLO adapter (M6) are next. See [docs/poc-design.md](docs/poc-design.md).

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
server/      Rust server (curve-native tracking backend)
clients/     Python SDK + CLI + framework adapters   (coming with M4)
deploy/      docker-compose / deployment              (coming with M5)
docs/        design + architecture documentation
scripts/     dev tooling (e.g. YOLO adapter validation probe)
examples/    usage examples                            (coming with M4)
```

## Quick start (server)

```bash
cd server
cargo run            # listens on 0.0.0.0:8080, creates ./taro.db
curl localhost:8080/health
```

See [server/README.md](server/README.md) for endpoints and configuration.

## Documentation

- [POC design spec](docs/poc-design.md) — data model, wire contract, milestones
- [Architecture](docs/architecture.md) — system diagrams + CLI design
- [Airflow integration](docs/airflow-integration.md) — future / not in POC

## License

Licensed under the [Apache License 2.0](LICENSE).
