# Taro Server

Curve-native experiment tracker — POC. Rust (axum + sqlx + SQLite).

See [../docs/poc-design.md](../docs/poc-design.md) for the full spec and
[../docs/architecture.md](../docs/architecture.md) for diagrams.

## Status: M1 — server skeleton
Implemented: health check, experiment get-or-create/list/get, run start/detail/finalize
(params + tags), embedded migrations, static bearer-token auth stub.
Not yet: scalar metrics (M2), curve metrics + compare (M3), artifacts (M5).

## Run

```bash
cp .env.example .env      # optional; defaults work out of the box
cargo run                 # listens on 0.0.0.0:8080, creates ./taro.db
```

Config via env (see `.env.example`): `TARO_DATABASE_URL`, `TARO_BIND`, `TARO_API_KEY`
(unset = auth disabled), `RUST_LOG`.

## Endpoints (M1)

| Method | Path | Body / notes |
|---|---|---|
| GET | `/health` | liveness |
| POST | `/api/v1/experiments` | `{ "name": "..." }` → get-or-create |
| GET | `/api/v1/experiments` | list |
| GET | `/api/v1/experiments/{id}` | detail |
| POST | `/api/v1/runs` | `{ "experiment", "name?", "params?", "tags?" }` → starts run |
| GET | `/api/v1/runs/{id}` | run + params + tags |
| PATCH | `/api/v1/runs/{id}` | `{ "status", "ended_at?" }` → finalize |

If `TARO_API_KEY` is set, send `Authorization: Bearer <key>` on `/api/v1/*`.

## Quick check

```bash
curl localhost:8080/health
curl -X POST localhost:8080/api/v1/runs -H 'content-type: application/json' \
  -d '{"experiment":"yolo-vehicle","name":"v1","params":{"lr0":0.01},"tags":{"dataset":"v4"}}'
```

## Layout

```
migrations/0001_init.sql   full 7-table schema (M1 uses experiment/run/param/tag)
src/
  main.rs                  entrypoint
  config.rs  state.rs  db.rs  error.rs  auth.rs
  models.rs                rows + request/response DTOs
  repo.rs                  data-access functions (trait extraction = later)
  api/{mod,health,experiments,runs}.rs
```
