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
        if path == "/runs/r1":
            return {
                "id": "r1", "experiment_id": "e1", "name": "run-a", "status": "finished",
                "started_at": "t0", "ended_at": "t1",
                "params": {"lr0": 0.01}, "tags": {"owner": "stelios"},
            }
        if path == "/runs/r1/metrics":
            return {"run_id": "r1", "series": {"mAP50": [{"step": 1, "value": 0.6, "ts": "t"}]}}
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
        raise AssertionError(f"unexpected GET {path}")

    def post(self, path, body, params=None):
        FakeClient.last = {"verb": "POST", "path": path, "body": body}
        assert path == "/experiments"
        return {"id": "e2", "name": body["name"], "created_at": "t"}

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


def test_compare_overlay(capsys):
    cli.main(["compare", "r1,r2", "--key", "pr_curve"])
    out = capsys.readouterr().out
    assert "pr_curve" in out and "r1" in out and "r2" in out
    assert FakeClient.last["params"]["step"] == "latest"


def test_server_error_exits_nonzero(monkeypatch):
    class Boom(FakeClient):
        def get(self, *a, **k):
            raise TaroHTTPError(404, "run not found")

    monkeypatch.setattr(cli, "Client", Boom)
    with pytest.raises(SystemExit) as exc:
        cli.main(["runs", "get", "missing"])
    assert exc.value.code == 1
