#!/usr/bin/env python3
"""
Taro — YOLO adapter validation probe.

Goal: de-risk the highest-uncertainty integration BEFORE writing the adapter.
Ultralytics' internal attribute paths to metrics, PR/per-class curves, and the
trainer callback object vary by version. This script discovers and reports the
*actual* paths in YOUR installed Ultralytics, and maps each to the field Taro's
wire format needs.

It does NOT write to a Taro server. It only introspects Ultralytics and prints a
PASS/FAIL report so you know exactly what the adapter can rely on.

Usage:
    python validate_yolo_adapter.py            # val()-based introspection (fast)
    python validate_yolo_adapter.py --train    # also run 1 epoch to probe the
                                               # on_fit_epoch_end trainer object
    python validate_yolo_adapter.py --json      # machine-readable report

Notes:
    - Downloads yolov8n.pt (~6MB) and the tiny coco8 dataset on first run.
    - Runs on CPU fine; coco8 is 8 images.
"""
from __future__ import annotations

import argparse
import json
import sys
from typing import Any, Callable


# ---- what Taro needs from Ultralytics, and where we expect to find it --------
# Each probe resolves a value from a metrics/trainer object via a getter, then we
# report whether it exists, its python type, and (if array-like) its shape.
#
# These getters encode our CURRENT assumption of the Ultralytics API. The whole
# point of the probe is to confirm/refute them against the installed version.

def _shape(v: Any) -> str:
    for attr in ("shape",):
        s = getattr(v, attr, None)
        if s is not None:
            return f"shape={tuple(s)}"
    if isinstance(v, (list, tuple)):
        n = len(v)
        inner = ""
        if n and isinstance(v[0], (list, tuple)):
            inner = f"x{len(v[0])}"
        return f"len={n}{inner}"
    if isinstance(v, dict):
        return f"dict(keys={list(v)[:6]}{'…' if len(v) > 6 else ''})"
    return ""


def probe(label: str, taro_field: str, getter: Callable[[], Any]) -> dict:
    """Run one getter defensively and capture the outcome."""
    try:
        v = getter()
    except Exception as e:  # attribute path wrong / not present in this version
        return {
            "label": label, "taro_field": taro_field, "ok": False,
            "type": None, "detail": f"{type(e).__name__}: {e}",
        }
    if v is None:
        return {"label": label, "taro_field": taro_field, "ok": False,
                "type": None, "detail": "resolved to None"}
    return {
        "label": label, "taro_field": taro_field, "ok": True,
        "type": type(v).__name__, "detail": _shape(v),
    }


# ---- val()-based introspection ----------------------------------------------
def probe_val(report: list[dict]) -> None:
    from ultralytics import YOLO

    print("· loading yolov8n.pt and running val() on coco8 …", file=sys.stderr)
    model = YOLO("yolov8n.pt")
    m = model.val(data="coco8.yaml", verbose=False)  # -> DetMetrics for detection

    # Scalar metrics Taro logs as scalar_metric. results_dict is the stable-ish
    # summary mapping; box.* are the typed accessors.
    report.append(probe("results_dict", "scalar_metric (summary dict)",
                        lambda: m.results_dict))
    report.append(probe("box.map50", "scalar_metric mAP50",
                        lambda: float(m.box.map50)))
    report.append(probe("box.map", "scalar_metric mAP50-95",
                        lambda: float(m.box.map)))
    report.append(probe("box.mp / box.mr", "scalar_metric precision/recall (mean)",
                        lambda: (float(m.box.mp), float(m.box.mr))))

    # Per-class AP -> Taro curve_metric (curve_type="per_class", x=class, y=AP).
    report.append(probe("box.maps", "curve_metric per_class_ap (y array)",
                        lambda: m.box.maps))
    report.append(probe("box.ap_class_index", "per_class_ap (class index order)",
                        lambda: m.box.ap_class_index))
    report.append(probe("names", "per_class labels",
                        lambda: m.names))

    # PR (and other) curves -> Taro curve_metric (curve_type="pr", x/y arrays).
    # In recent Ultralytics: m.curves is a list of names, m.curves_results is a
    # parallel list of [x, y, x_label, y_label]; PR curve y is (num_classes, N).
    report.append(probe("curves", "curve names list",
                        lambda: m.curves))
    report.append(probe("curves_results", "curve payloads [x, y, xlabel, ylabel]",
                        lambda: m.curves_results))

    # Try to pull the PR curve specifically and show its decomposition, since
    # that is the exact thing the adapter maps to log_curve().
    def _pr_curve():
        names = list(m.curves)
        idx = next(i for i, n in enumerate(names) if "Precision-Recall" in n)
        x, y, xl, yl = m.curves_results[idx]
        return {"x_label": xl, "y_label": yl, "x_len": len(x),
                "y_shape": tuple(getattr(y, "shape", (len(y),)))}
    report.append(probe("curves_results[PR]", "pr_curve -> log_curve(x,y)", _pr_curve))


# ---- callback / trainer introspection (requires --train) ---------------------
def probe_train(report: list[dict]) -> None:
    from ultralytics import YOLO

    print("· running 1 epoch on coco8 to probe on_fit_epoch_end trainer …",
          file=sys.stderr)
    captured: dict[str, Any] = {}

    def on_fit_epoch_end(trainer):  # signature Ultralytics calls us with
        # Capture once; this is the object the adapter will read each epoch.
        if "trainer" not in captured:
            captured["trainer"] = trainer

    model = YOLO("yolov8n.pt")
    model.add_callback("on_fit_epoch_end", on_fit_epoch_end)
    model.train(data="coco8.yaml", epochs=1, imgsz=320, verbose=False, plots=False)

    t = captured.get("trainer")
    if t is None:
        report.append({"label": "on_fit_epoch_end", "taro_field": "callback fired",
                       "ok": False, "type": None, "detail": "callback never called"})
        return

    report.append(probe("trainer.epoch", "curve/metric step",
                        lambda: t.epoch))
    report.append(probe("trainer.metrics", "scalar_metric dict per epoch",
                        lambda: t.metrics))
    report.append(probe("trainer.validator.metrics", "DetMetrics for curves",
                        lambda: t.validator.metrics))
    report.append(probe("trainer.best", "artifact best.pt path",
                        lambda: t.best))


# ---- reporting ---------------------------------------------------------------
def print_report(report: list[dict]) -> int:
    width = max((len(r["taro_field"]) for r in report), default=10)
    print("\n=== Taro YOLO adapter validation ===\n")
    for r in report:
        mark = "PASS" if r["ok"] else "FAIL"
        line = f"[{mark}] {r['taro_field']:<{width}}  ← {r['label']}"
        if r["type"]:
            line += f"  ({r['type']} {r['detail']})".rstrip()
        else:
            line += f"  — {r['detail']}"
        print(line)
    n_fail = sum(1 for r in report if not r["ok"])
    print(f"\n{len(report) - n_fail}/{len(report)} checks passed.")
    if n_fail:
        print("\nFAILed checks mean the adapter must use a different attribute "
              "path for that field in your Ultralytics version — update the "
              "getter and re-run before building taro.integrations.ultralytics.")
    return 1 if n_fail else 0


def main() -> int:
    ap = argparse.ArgumentParser(description="Validate YOLO->Taro adapter assumptions")
    ap.add_argument("--train", action="store_true",
                    help="also run 1 epoch to probe the on_fit_epoch_end trainer")
    ap.add_argument("--json", action="store_true", help="emit JSON report")
    args = ap.parse_args()

    try:
        import ultralytics
        print(f"· ultralytics {ultralytics.__version__}", file=sys.stderr)
    except ImportError:
        print("ERROR: ultralytics not installed. Run inside your training env:\n"
              "  pip install ultralytics", file=sys.stderr)
        return 2

    report: list[dict] = []
    probe_val(report)
    if args.train:
        probe_train(report)

    if args.json:
        import ultralytics
        print(json.dumps({"ultralytics": ultralytics.__version__,
                          "checks": report}, indent=2))
        return 1 if any(not r["ok"] for r in report) else 0
    return print_report(report)


if __name__ == "__main__":
    sys.exit(main())
