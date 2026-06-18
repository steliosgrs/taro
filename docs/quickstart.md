# Taro — Quickstart (build a PoC)

For a developer wiring Taro into a real training script for the first time. Goal:
get runs logging in ~10 minutes, then overlay two runs' curves — the thing Taro
exists for.

You interact with exactly **two pieces**: a **server** you start once, and the
**Python SDK** you import into your training code. You don't need to read the
Rust. Canonical spec: `poc-design.md`.

---

## 0. Prerequisites

- **Rust** (to run the server): `cargo` on your PATH. `cargo run` builds it.
- **Python 3.9+** for the SDK. The SDK core is **stdlib-only** (zero deps).
- That's it for a laptop PoC — SQLite + local files, no external services.

---

## 1. Start the server

```bash
cd server
cargo run                      # http://0.0.0.0:8080
```

On first run it creates `taro.db` (SQLite) and `./taro_blobs/` (artifact bytes)
and applies migrations automatically. Check it's up:

```bash
curl localhost:8080/health     # {"status":"ok",...}
```

Leave it running in its own terminal.

---

## 2. Install the SDK

```bash
cd clients/python
uv venv && uv pip install -e .          # core: zero deps
# YOLO adapter / examples also need: uv pip install ultralytics
```

(`pip install -e .` works too if you're not using `uv`.)

---

## 3. Instrument your training

The shape is always the same: `init` once, then a `start_run` block per run.

```python
import taro

taro.init("http://localhost:8080")                 # once per process

with taro.start_run("my-experiment", params={"lr": 0.01, "epochs": 50}) as run:
    for epoch in range(50):
        # ... your training step ...
        run.log_metric("loss", loss_value, step=epoch)      # a scalar
        run.log_metric("accuracy", acc_value, step=epoch)

    # the payoff: log a curve as structured data, not a screenshot
    run.log_curve(
        "pr_curve", x=recall, y=precision, step=epoch,
        curve_type="pr", x_label="recall", y_label="precision",
    )

    run.log_artifact("checkpoints/best.pt")          # uploads the file's bytes
```

- `params` are hyperparameters (immutable); `tags` are mutable labels.
- `step` is any monotonic integer you choose (epoch, global iteration — your
  call). The server treats it as opaque; it only orders by it.
- The `with` block **auto-finalizes** the run: `finished` on a clean exit,
  `failed` if your code raises (and the exception still propagates).

### Multi-line curves (e.g. per-class)

Pass `series` instead of `y` — a list of `{"name", "y"}` sharing the same `x`:

```python
run.log_curve("pr_per_class", x=recall, curve_type="pr", series=[
    {"name": "cat", "y": precision_cat},
    {"name": "dog", "y": precision_dog},
])
```

---

## 4. Overlay runs — the reason Taro exists

After two or more runs have logged the same curve `key`, fetch them as
**comparable data** (not an image) and plot however you like:

```python
overlay = taro.compare_curves([run_a_id, run_b_id], key="pr_curve")
# -> {"key": "pr_curve", "x_label": ..., "y_label": ...,
#     "runs": [{"run_id", "run_name", "step", "data": {"x", "y"}}, ...]}

import matplotlib.pyplot as plt
for r in overlay["runs"]:
    plt.plot(r["data"]["x"], r["data"]["y"], label=r["run_name"] or r["run_id"])
plt.legend(); plt.show()
```

By default it grabs each run's **latest** step for that key; pass `step=<n>` to
pin a specific one. Runs missing the curve are silently skipped.

---

## 5. Using a framework adapter (YOLO)

If you train Ultralytics YOLO, you don't hand-log — attach the adapter and it
logs scalars, PR / per-class curves, and `best.pt` for you:

```python
import taro
from taro.integrations.ultralytics import attach

taro.init("http://localhost:8080")
model = YOLO("yolov8n.pt")
attach(model, experiment="vehicle-detector", params={"imgsz": 640})
model.train(data="coco8.yaml", epochs=50)           # logging happens via callbacks
```

For other frameworks (torch, sklearn, XGBoost), call the SDK directly as in §3.

---

## 6. Things that will save you debugging time

1. **Taro never crashes your training.** If the server is unreachable, the run
   degrades to a no-op (`run.ok is False`) and logging is silently skipped — your
   model still trains. So *"it ran but logged nothing"* almost always means the
   **server wasn't up** or `init` pointed at the wrong URL. Check `run.ok`.
2. **Only a `running` run accepts data.** Once a run finishes it's immutable;
   logging to it returns an error (which the SDK swallows). Don't reuse a run id
   across processes.
3. **Scalars are batched, curves/artifacts are immediate.** Scalars flush on a
   background thread (on count or time); they're guaranteed flushed when the run
   finishes. If you skip the `with` block, call `run.finish()` yourself.
4. **Curves validate structure only.** Equal lengths, finite numbers, exactly one
   of `y`/`series`. The server never judges ML correctness — a "wrong" curve that
   is structurally valid is stored as-is.

---

## 7. Configuration (server env vars, all optional)

| Var | Default | Purpose |
|---|---|---|
| `TARO_DATABASE_URL` | `sqlite://taro.db` | `postgres://…` for a shared/prod DB |
| `TARO_BIND` | `0.0.0.0:8080` | listen address |
| `TARO_API_KEY` | unset (auth off) | require `Authorization: Bearer <key>` |
| `TARO_BLOB_ROOT` | `./taro_blobs` | where artifact bytes are stored |

If you set `TARO_API_KEY`, pass it from the SDK: `taro.init(url, api_key="…")`.

To point a PoC at Postgres instead of SQLite, just change the URL — same server,
same SDK code:

```bash
TARO_DATABASE_URL=postgres://user:pass@host:5432/taro cargo run
```

---

## 8. Verify your setup

With the server up, run the bundled smoke test — it logs two fake PR curves
through the SDK and overlays them:

```bash
cd clients/python
python examples/validate_m3.py
```

---

## Where to go next

- `poc-design.md` — the full wire contract and data model.
- `architecture.md` — diagrams + component boundaries.
- REST API is at `/api/v1` if you'd rather call it directly (any language).
