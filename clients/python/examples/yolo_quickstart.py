"""Log a real Ultralytics YOLO run to Taro (M6).

Requires `pip install ultralytics` and a running Taro server. The adapter logs
per-epoch scalars, PR / per-class-AP curves, and best.pt — then two such runs are
comparable via /curves/compare.

    cd server && cargo run                                  # terminal 1
    python clients/python/examples/yolo_quickstart.py        # terminal 2
"""

import taro
from taro.integrations.ultralytics import attach


def main() -> None:
    from ultralytics import YOLO

    taro.init("http://localhost:8080")

    model = YOLO("yolov8n.pt")
    run = attach(model, "yolo-vehicle-detector", name="yolov8n-coco8",
                 params={"model": "yolov8n", "imgsz": 320})

    # Taro callbacks fire during training; the run auto-finalizes on train end.
    model.train(data="coco8.yaml", epochs=3, imgsz=320, plots=False)

    overlay = taro.compare_curves([run.run_id], key="pr_curve")
    print("PR curve points logged:", len(overlay["runs"][0]["data"]["x"]))


if __name__ == "__main__":
    main()
