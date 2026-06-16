"""Ultralytics YOLO → Taro adapter (M6).

`attach(model, experiment=...)` registers callbacks that log per-epoch scalars,
PR / per-class-AP curves, and the best weights — turning a normal YOLO training
run into comparable Taro curves. All Ultralytics-specific extraction lives here;
the SDK core stays framework-agnostic.

The attribute paths below (`trainer.metrics`, `validator.metrics.curves_results`,
`box.maps`, `trainer.best`, …) are version-sensitive — they are exactly what
`scripts/validate_yolo_adapter.py` confirms for your installed version. Every
callback is wrapped so a wrong path warns and continues: **tracking never crashes
training.**
"""

import logging
import os
from typing import Any, List, Optional

from .. import start_run
from ..run import Run

log = logging.getLogger("taro")


def attach(
    model: Any,
    experiment: str,
    *,
    name: Optional[str] = None,
    params: Optional[dict] = None,
    tags: Optional[dict] = None,
    log_weights: bool = True,
    log_curves: bool = True,
    client: Optional[Any] = None,
    run: Optional[Run] = None,
) -> Run:
    """Attach Taro logging to a YOLO model. Returns the (started) `Run`, which is
    finalized automatically on training end. Pass your own `run` to manage its
    lifecycle yourself."""
    owns_run = run is None
    run = run or start_run(experiment, name=name, params=params, tags=tags, client=client)

    def on_fit_epoch_end(trainer: Any) -> None:
        try:
            step = int(getattr(trainer, "epoch", 0))
            _log_scalars(run, trainer, step)
            if log_curves:
                _log_curves(run, _validator_metrics(trainer), step)
        except Exception as e:  # never-crash: extraction must not kill training
            log.warning("taro: ultralytics epoch logging failed (%s)", e)

    def on_train_end(trainer: Any) -> None:
        try:
            best = getattr(trainer, "best", None)
            if log_weights and best and os.path.exists(str(best)):
                run.log_artifact(str(best), name="best.pt")
        except Exception as e:
            log.warning("taro: ultralytics weight logging failed (%s)", e)
        finally:
            if owns_run:
                run.finish("finished")

    model.add_callback("on_fit_epoch_end", on_fit_epoch_end)
    model.add_callback("on_train_end", on_train_end)
    return run


# ----- extraction (version-sensitive; see the validation probe) ---------------

def _log_scalars(run: Run, trainer: Any, step: int) -> None:
    # trainer.metrics is a flat {key: float} dict, e.g. "metrics/mAP50(B)".
    for key, value in (getattr(trainer, "metrics", None) or {}).items():
        try:
            run.log_metric(str(key), float(value), step=step)
        except (TypeError, ValueError):
            continue  # skip non-numeric entries


def _validator_metrics(trainer: Any) -> Any:
    return getattr(getattr(trainer, "validator", None), "metrics", None)


def _log_curves(run: Run, metrics: Any, step: int) -> None:
    if metrics is None:
        return
    _log_pr_curve(run, metrics, step)
    _log_per_class_ap(run, metrics, step)


def _log_pr_curve(run: Run, metrics: Any, step: int) -> None:
    names = list(getattr(metrics, "curves", None) or [])
    results = getattr(metrics, "curves_results", None)
    if not names or not results:
        return
    idx = next((i for i, n in enumerate(names) if "Precision-Recall" in str(n)), None)
    if idx is None:
        return

    x, y, x_label, y_label = results[idx]
    x = _to_list(x)
    rows = _to_2d(y)  # PR y is typically (num_classes, N)
    class_names = _class_names(metrics)
    idxs = _to_list(getattr(getattr(metrics, "box", None), "ap_class_index", None))

    if len(rows) == 1:
        run.log_curve("pr_curve", x=x, y=rows[0], step=step,
                      curve_type="pr", x_label=x_label, y_label=y_label)
        return

    # Mean curve = the run's headline PR; per-class as a multi-line overlay.
    mean_y = [sum(col) / len(col) for col in zip(*rows)]
    run.log_curve("pr_curve", x=x, y=mean_y, step=step,
                  curve_type="pr", x_label=x_label, y_label=y_label)
    series = [
        {"name": _name(class_names, int(idxs[i]) if i < len(idxs) else i), "y": row}
        for i, row in enumerate(rows)
    ]
    run.log_curve("pr_curve_per_class", x=x, series=series, step=step,
                  curve_type="pr", x_label=x_label, y_label=y_label)


def _log_per_class_ap(run: Run, metrics: Any, step: int) -> None:
    box = getattr(metrics, "box", None)
    maps = _to_list(getattr(box, "maps", None))  # per-class mAP50-95, indexed by class id
    if not maps:
        return
    idxs = _to_list(getattr(box, "ap_class_index", None))
    class_names = _class_names(metrics)

    present = [int(i) for i in idxs] if idxs else list(range(len(maps)))
    y = [float(maps[i]) for i in present if i < len(maps)]
    labels = [_name(class_names, i) for i in present[: len(y)]]
    run.log_curve("per_class_ap", x=list(range(len(y))), y=y, step=step,
                  curve_type="per_class", x_label="class", y_label="AP", labels=labels)


# ----- small conversion helpers (numpy/torch tensors → plain python) ----------

def _to_list(v: Any) -> List[Any]:
    if v is None:
        return []
    if hasattr(v, "tolist"):
        v = v.tolist()
    return list(v)


def _to_2d(y: Any) -> List[List[float]]:
    if hasattr(y, "tolist"):
        y = y.tolist()
    if y and isinstance(y[0], (list, tuple)):
        return [list(row) for row in y]
    return [list(y)]


def _class_names(metrics: Any) -> Any:
    return getattr(metrics, "names", None)


def _name(names: Any, i: int) -> str:
    if isinstance(names, dict):
        return str(names.get(i, i))
    if isinstance(names, (list, tuple)) and i < len(names):
        return str(names[i])
    return str(i)
