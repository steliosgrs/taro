"""End-to-end round-trips against a live server (opt-in).

Use the `server_url` fixture — these skip unless TARO_TEST_SERVER_URL is set.
Start the server, then:  TARO_TEST_SERVER_URL=http://localhost:8080 pytest -m integration
"""

import pytest

import taro


@pytest.mark.integration
def test_round_trip(server_url):
    client = taro.init(server_url)
    with taro.start_run("pytest-roundtrip", params={"lr0": 0.01}) as run:
        assert run.ok
        run_id = run.run_id
        run.log_metric("mAP50", 0.64, step=1)
        run.log_metric("mAP50", 0.70, step=2)
        run.log_curve("pr_curve", x=[0.0, 0.5, 1.0], y=[1.0, 0.8, 0.6],
                      step=1, curve_type="pr", x_label="recall", y_label="precision")
        run.register_artifact("weights", "s3://bucket/best.pt", size_bytes=10)
    # finish() flushed the batched scalars; read everything back.

    metrics = client.get(f"/runs/{run_id}/metrics")
    assert [p["step"] for p in metrics["series"]["mAP50"]] == [1, 2]

    curves = client.get(f"/runs/{run_id}/curves", {"key": "pr_curve"})["curves"]
    assert len(curves) == 1
    assert curves[0]["data"]["y"] == [1.0, 0.8, 0.6]

    artifacts = client.get(f"/runs/{run_id}/artifacts")
    assert any(a["name"] == "weights" for a in artifacts)

    detail = client.get(f"/runs/{run_id}")
    assert detail["status"] == "finished"


@pytest.mark.integration
def test_numpy_values_serialize(server_url):
    # Regression guard: numpy scalars/arrays must be converted to plain python
    # before logging, or json.dumps would choke. log_metric does float(); array
    # callers pass .tolist().
    np = pytest.importorskip("numpy")
    client = taro.init(server_url)
    with taro.start_run("pytest-numpy") as run:
        run_id = run.run_id
        run.log_metric("acc", np.float64(0.875), step=1)  # float() inside log_metric
        recall = np.linspace(0.0, 1.0, 5)
        precision = np.linspace(1.0, 0.0, 5)
        run.log_curve("pr_curve", x=recall.tolist(), y=precision.tolist(),
                      step=1, curve_type="pr")

    metrics = client.get(f"/runs/{run_id}/metrics")
    assert metrics["series"]["acc"][0]["value"] == 0.875
    curves = client.get(f"/runs/{run_id}/curves", {"key": "pr_curve"})["curves"]
    assert len(curves[0]["data"]["x"]) == 5


# ----- YOLO adapter against fake trainer objects (promotes the M6 smoke test) --

class _FakeBox:
    def __init__(self):
        self.maps = [0.5, 0.7]          # per-class mAP50-95, indexed by class id
        self.ap_class_index = [0, 1]


class _FakeDetMetrics:
    def __init__(self):
        self.curves = ["Precision-Recall(B)"]
        x = [0.0, 0.5, 1.0]
        y = [[1.0, 0.8, 0.6], [0.9, 0.7, 0.5]]  # (num_classes, N)
        self.curves_results = [[x, y, "Recall", "Precision"]]
        self.box = _FakeBox()
        self.names = {0: "cat", 1: "dog"}


class _FakeValidator:
    def __init__(self):
        self.metrics = _FakeDetMetrics()


class _FakeTrainer:
    def __init__(self, best):
        self.epoch = 0
        self.metrics = {"metrics/mAP50(B)": 0.64, "metrics/mAP50-95(B)": 0.45}
        self.validator = _FakeValidator()
        self.best = best


class _FakeModel:
    def __init__(self):
        self.callbacks = {}

    def add_callback(self, name, fn):
        self.callbacks[name] = fn


@pytest.mark.integration
def test_ultralytics_adapter_with_fakes(server_url, tmp_path):
    from taro.integrations.ultralytics import attach

    client = taro.init(server_url)
    best = tmp_path / "best.pt"
    best.write_bytes(b"fake-weights")

    model = _FakeModel()
    run = attach(model, "pytest-yolo", params={"imgsz": 320}, client=client)
    assert run.ok
    run_id = run.run_id

    # Drive the callbacks the way Ultralytics would.
    trainer = _FakeTrainer(str(best))
    model.callbacks["on_fit_epoch_end"](trainer)  # scalars + curves
    model.callbacks["on_train_end"](trainer)       # best.pt + auto-finish

    # A multi-class run emits exactly these three curve keys.
    curves = client.get(f"/runs/{run_id}/curves")["curves"]
    keys = {c["key"] for c in curves}
    assert {"pr_curve", "pr_curve_per_class", "per_class_ap"} <= keys

    # per_class_ap carries the class labels.
    ap = next(c for c in curves if c["key"] == "per_class_ap")
    assert ap["data"]["labels"] == ["cat", "dog"]

    # best.pt uploaded and the run auto-finalized on train end.
    artifacts = client.get(f"/runs/{run_id}/artifacts")
    assert any(a["name"] == "best.pt" for a in artifacts)
    assert client.get(f"/runs/{run_id}")["status"] == "finished"
