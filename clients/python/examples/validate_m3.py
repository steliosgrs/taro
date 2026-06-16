"""Validate M3 end-to-end through the SDK: log two fake PR curves, then overlay.

This is the POC success criterion in miniature (no YOLO yet) — two runs whose PR
curves come back from /curves/compare as data, the thing MLflow cannot do.

    cd server && cargo run            # terminal 1 (port 8080)
    python clients/python/examples/validate_m3.py   # terminal 2
"""

import logging

import taro

logging.basicConfig(level=logging.INFO, format="%(message)s")


def fake_pr_curve(drop: float):
    """recall 0..1; precision starts ~1 and decays. `drop` shifts the tradeoff."""
    x = [i / 10 for i in range(11)]
    y = [round(max(0.0, 1.0 - drop * r * r), 4) for r in x]
    return x, y


def main() -> None:
    taro.init("http://localhost:8080")

    run_ids = []
    for name, drop in [("yolov8n-v3", 0.5), ("yolov8n-v2", 0.7)]:
        with taro.start_run(
            "yolo-vehicle-detector", name=name,
            params={"model": "yolov8n", "drop": drop},
        ) as run:
            if not run.ok:
                raise SystemExit("could not start run — is the server up on :8080?")
            x, y = fake_pr_curve(drop)
            for step in (50, 100):
                run.log_metric("mAP50", y[5] + step / 1000, step=step)
                run.log_curve(
                    "pr_curve", x=x, y=y, step=step, curve_type="pr",
                    x_label="recall", y_label="precision",
                )
            run_ids.append(run.run_id)
            print(f"logged {name}: {run.run_id}")

    overlay = taro.compare_curves(run_ids, key="pr_curve", step="latest")
    print("\n/curves/compare (latest):")
    for r in overlay["runs"]:
        d = r["data"]
        print(f"  {r['run_name']:<12} step={r['step']:<4} {len(d['x'])} pts  "
              f"P@R=0.5={d['y'][5]:.3f}")

    assert len(overlay["runs"]) == 2, "expected both runs in the overlay"
    print("\nM3 validated through the SDK: two PR curves overlaid as data. ✅")


if __name__ == "__main__":
    main()
