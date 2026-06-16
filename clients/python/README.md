# Taro Python SDK (M4)

Ergonomic, framework-agnostic logging client over the Taro REST API. Standard
library only (no runtime dependencies) for the POC.

```python
import taro

taro.init("http://localhost:8080")
with taro.start_run("yolo-vehicle-detector", params={"lr0": 0.01}) as run:
    run.log_metric("mAP50", 0.64, step=50)
    run.log_curve("pr_curve", x=recall, y=precision, step=50,
                  curve_type="pr", x_label="recall", y_label="precision")

overlay = taro.compare_curves([run_a, run_b], key="pr_curve")  # data, not a PNG
```

## Design

- **`Run` is a context manager** → auto-finalizes (`finished`, or `failed` on an
  exception, which is re-raised — tracking never swallows your error).
- **Scalars are batched** in a background thread (flush on size or time); **curves
  flush immediately** (low frequency, and you want them durable).
- **Never crash training.** A tracking failure (server down, bad response) logs a
  warning and continues. If a run can't even be created, you get a *degraded*
  no-op run (`run.ok is False`) and the loop runs untracked.

## Status / scope

- M4 (this): `init`, `start_run`, `log_metric`, `log_curve`, `compare_curves`.
- Artifacts (`log_artifact`) land in **M5**; framework adapters
  (`taro.integrations.*`) in **M6**.

## Try it

```bash
cd server && cargo run                              # terminal 1
python clients/python/examples/validate_m3.py        # terminal 2
```
