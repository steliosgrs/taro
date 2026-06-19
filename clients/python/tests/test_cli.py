"""CLI tests (M10) — parsing + rendering, no live server.

A `FakeClient` returns canned responses for each endpoint; we patch
`taro.cli.Client` so `main()` builds the fake. Asserts cover human tables, the
`--json` passthrough, and the non-zero exit on a server error.
"""

import json

import pytest

import taro.cli as cli
from taro._client import TaroHTTPError


class FakeClient:
    """Stands in for taro._client.Client; records calls, returns canned data."""

    last: dict = {}

    def __init__(self, *args, **kwargs):
        FakeClient.last = {}

    def get(self, path, params=None):
        FakeClient.last = {"verb": "GET", "path": path, "params": params}
        if path == "/experiments":
            return [{"id": "e1", "name": "exp", "created_at": "2026-06-19T00:00:00Z"}]
        if path == "/experiments/e1":
            return {"id": "e1", "name": "exp", "created_at": "2026-06-19T00:00:00Z"}
        if path == "/runs":
            return [
                {"id": "r2", "experiment_id": "e1", "name": "run-b", "status": "running",
                 "started_at": "t2", "ended_at": None},
                {"id": "r1", "experiment_id": "e1", "name": "run-a", "status": "finished",
                 "started_at": "t0", "ended_at": "t1"},
            ]
        if path == "/runs/r1":
            return {
                "id": "r1", "experiment_id": "e1", "name": "run-a", "status": "finished",
                "started_at": "t0", "ended_at": "t1",
                "params": {"lr0": 0.01, "epochs": 50}, "tags": {"owner": "stelios"},
            }
        if path == "/runs/r2":
            return {
                "id": "r2", "experiment_id": "e1", "name": "run-b", "status": "running",
                "started_at": "t2", "ended_at": None,
                "params": {"lr0": 0.02, "epochs": 50}, "tags": {"owner": "stelios"},
            }
        if path == "/runs/r1/metrics":
            return {"run_id": "r1", "series": {"mAP50": [
                {"step": 1, "value": 0.6, "ts": "t"}, {"step": 2, "value": 0.64, "ts": "t"}]}}
        if path == "/runs/r2/metrics":
            return {"run_id": "r2", "series": {"mAP50": [{"step": 1, "value": 0.71, "ts": "t"}]}}
        if path == "/runs/r1/curves":
            return {"run_id": "r1", "curves": [
                {"key": "pr_curve", "step": 1, "curve_type": "pr", "x_label": "recall",
                 "y_label": "precision", "data": {"x": [0, 0.5, 1], "y": [1, 0.8, 0.6]}, "ts": "t"},
            ]}
        if path == "/runs/r1/artifacts":
            return [{"id": "a1", "name": "best.pt", "media_type": "application/octet-stream",
                     "size_bytes": 1024, "uri": "file:///blobs/best.pt", "created_at": "t"}]
        if path == "/curves/compare":
            return {"key": "pr_curve", "x_label": "recall", "y_label": "precision", "runs": [
                {"run_id": "r1", "run_name": "a", "step": 5, "data": {"x": [0, 1], "y": [1, 0]}},
                {"run_id": "r2", "run_name": "b", "step": 7, "data": {"x": [0, 1], "y": [1, 0]}},
            ]}
        if path == "/documents":
            return [{"id": "d1", "namespace": "config", "name": "yolo", "created_at": "t"}]
        if path == "/documents/d1":
            return {"id": "d1", "namespace": "config", "name": "yolo", "created_at": "t",
                    "versions": [
                        {"id": "v1", "version": 1, "content_hash": "abc123",
                         "parent_version_id": None, "created_at": "t"}]}
        if path == "/versions/v1":
            return {"id": "v1", "document_id": "d1", "version": 1, "content_hash": "abc123",
                    "body": {"lr0": 0.01}, "parent_version_id": None, "created_at": "t"}
        if path == "/versions/v1/runs":
            return [{"id": "r1", "experiment_id": "e1", "name": "run-a", "status": "finished",
                     "started_at": "t0", "ended_at": "t1"}]
        if path == "/runs/r1/documents":
            return [{"role": "config", "id": "v1", "document_id": "d1", "version": 1,
                     "content_hash": "abc123def456", "body": {"lr0": 0.01},
                     "parent_version_id": None, "created_at": "t"}]
        raise AssertionError(f"unexpected GET {path}")

    def post(self, path, body, params=None):
        FakeClient.last = {"verb": "POST", "path": path, "body": body}
        if path == "/experiments":
            return {"id": "e2", "name": body["name"], "created_at": "t"}
        if path == "/documents":
            return {"id": "d1", "namespace": body["namespace"], "name": body["name"],
                    "created_at": "t"}
        if path == "/documents/d1/versions":
            return {"version_id": "v1", "version": 1, "content_hash": "abc123def456",
                    "deduped": False}
        raise AssertionError(f"unexpected POST {path}")

    def health(self):
        return {"status": "ok", "service": "taro-server", "version": "0.1.0"}


@pytest.fixture(autouse=True)
def patch_client(monkeypatch):
    monkeypatch.setattr(cli, "Client", FakeClient)


def test_health(capsys):
    cli.main(["health"])
    assert "ok" in capsys.readouterr().out


def test_experiments_list_table(capsys):
    cli.main(["experiments", "list"])
    out = capsys.readouterr().out
    assert "exp" in out and "e1" in out and "NAME" in out


def test_json_flag_passthrough(capsys):
    cli.main(["--json", "experiments", "list"])
    parsed = json.loads(capsys.readouterr().out)
    assert parsed[0]["name"] == "exp"


def test_experiments_create(capsys):
    cli.main(["experiments", "create", "vehicle-detector"])
    out = capsys.readouterr().out
    assert "created experiment" in out
    assert FakeClient.last["body"] == {"name": "vehicle-detector"}


def test_run_get_shows_params_and_tags(capsys):
    cli.main(["runs", "get", "r1"])
    out = capsys.readouterr().out
    assert "finished" in out and "lr0 = 0.01" in out and "owner = stelios" in out


def test_run_curves_counts_points(capsys):
    cli.main(["runs", "curves", "r1", "--key", "pr_curve"])
    out = capsys.readouterr().out
    assert "pr_curve" in out and "3" in out  # 3 x-values
    assert FakeClient.last["params"] == {"key": "pr_curve", "step": None}


def test_runs_list_table_and_filters(capsys):
    cli.main(["runs", "list", "--experiment", "e1", "--status", "running", "--limit", "5"])
    out = capsys.readouterr().out
    assert "run-a" in out and "run-b" in out and "STATUS" in out
    # Filters ride along as query params (None-valued ones dropped by the client).
    assert FakeClient.last["params"] == {"experiment_id": "e1", "status": "running", "limit": 5}


def test_runs_diff_marks_differences(capsys):
    cli.main(["runs", "diff", "r1", "r2"])
    out = capsys.readouterr().out
    # lr0 differs (0.01 vs 0.02) and latest mAP50 differs (0.64 vs 0.71) → flagged;
    # epochs (50 vs 50) and the shared owner tag match → not flagged.
    assert "lr0" in out and "epochs" in out and "mAP50" in out
    assert "0.64" in out and "0.71" in out  # latest value per run, not step 1
    assert "*" in out


def test_compare_overlay(capsys):
    cli.main(["compare", "r1,r2", "--key", "pr_curve"])
    out = capsys.readouterr().out
    assert "pr_curve" in out and "r1" in out and "r2" in out
    assert FakeClient.last["params"]["step"] == "latest"


def test_documents_list_and_filter(capsys):
    cli.main(["documents", "list", "--namespace", "config"])
    out = capsys.readouterr().out
    assert "yolo" in out and "config" in out and "d1" in out
    assert FakeClient.last["params"] == {"namespace": "config", "name": None}


def test_documents_get_shows_versions(capsys):
    cli.main(["documents", "get", "d1"])
    out = capsys.readouterr().out
    assert "yolo" in out and "abc123" in out  # version history rendered


def test_documents_create(capsys):
    cli.main(["documents", "create", "config", "yolo-baseline"])
    out = capsys.readouterr().out
    assert "created document" in out
    assert FakeClient.last["body"] == {"namespace": "config", "name": "yolo-baseline"}


def test_documents_publish_from_file(tmp_path, capsys):
    f = tmp_path / "cfg.json"
    f.write_text(json.dumps({"lr0": 0.01, "epochs": 100}))
    cli.main(["documents", "publish", "d1", str(f)])
    out = capsys.readouterr().out
    assert "version 1 created" in out
    assert FakeClient.last["body"] == {"body": {"lr0": 0.01, "epochs": 100}}


def test_version_get_shows_body(capsys):
    cli.main(["versions", "get", "v1"])
    out = capsys.readouterr().out
    assert "abc123" in out and "lr0" in out  # body rendered


def test_version_runs_reverse_lookup(capsys):
    cli.main(["versions", "runs", "v1"])
    out = capsys.readouterr().out
    assert "r1" in out and "run-a" in out


def test_run_documents_forward(capsys):
    cli.main(["runs", "documents", "r1"])
    out = capsys.readouterr().out
    assert "config" in out and "v1" in out and "d1" in out


def test_publish_missing_file_exits_nonzero(monkeypatch):
    with pytest.raises(SystemExit) as exc:
        cli.main(["documents", "publish", "d1", "/no/such/file.json"])
    assert exc.value.code == 1


def test_server_error_exits_nonzero(monkeypatch):
    class Boom(FakeClient):
        def get(self, *a, **k):
            raise TaroHTTPError(404, "run not found")

    monkeypatch.setattr(cli, "Client", Boom)
    with pytest.raises(SystemExit) as exc:
        cli.main(["runs", "get", "missing"])
    assert exc.value.code == 1
