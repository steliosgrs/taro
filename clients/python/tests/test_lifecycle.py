"""Context-manager lifecycle.

The `with taro.start_run(...)` block must finalize the run on exit — "finished"
on a clean exit, "failed" on an exception — and must NOT swallow a training
exception (only tracking errors are swallowed). Driven with a recording client
so no server is needed.
"""

import pytest

import taro
from taro.run import Run


class RecordingClient:
    """Minimal Client stand-in that records PATCH (finalize) calls."""

    def __init__(self):
        self.patches = []

    def post(self, path, body, params=None):
        return {}

    def get(self, path, params=None):
        return {}

    def patch(self, path, body):
        self.patches.append((path, body))
        return {}

    def post_file(self, *args, **kwargs):
        return {}


def test_context_manager_finishes():
    client = RecordingClient()
    # Long interval so the batcher timer never fires mid-test; close() drains.
    with Run(client, "run-1", interval=3600) as run:
        run.log_metric("loss", 0.5, step=0)

    assert client.patches[-1] == ("/runs/run-1", {"status": "finished"})


def test_exception_marks_failed_and_reraises():
    client = RecordingClient()
    # The training exception must propagate out of the with-block...
    with pytest.raises(ValueError, match="boom"):
        with Run(client, "run-1", interval=3600):
            raise ValueError("boom")

    # ...while the run is still finalized as "failed".
    assert client.patches[-1] == ("/runs/run-1", {"status": "failed"})


class ConfigRecordingClient:
    """Records POSTs and returns canned registry responses (no server)."""

    def __init__(self):
        self.posts = []

    def post(self, path, body, params=None):
        self.posts.append((path, body))
        if path == "/documents":
            return {"id": "doc-1"}
        if path.endswith("/versions"):
            return {"version_id": "ver-1", "version": 1, "deduped": False}
        if path == "/runs":
            return {"run_id": "run-1", "experiment_id": "exp-1"}
        return {}

    def get(self, path, params=None):
        return {}


def test_register_config_publishes_and_returns_version_id():
    client = ConfigRecordingClient()
    vid = taro.register_config("yolo-baseline", {"lr0": 0.01}, client=client)

    assert vid == "ver-1"
    # get-or-create the handle, then publish the body under it.
    assert client.posts[0] == ("/documents", {"namespace": "config", "name": "yolo-baseline"})
    assert client.posts[1] == ("/documents/doc-1/versions", {"body": {"lr0": 0.01}})


def test_start_run_passes_config_version_id_inline():
    client = ConfigRecordingClient()
    taro.start_run("exp", config_version_id="ver-1", client=client, interval=3600)

    run_post = next(b for p, b in client.posts if p == "/runs")
    assert run_post["config_version_id"] == "ver-1"


def test_register_dataset_builds_recipe_body_with_lineage():
    client = ConfigRecordingClient()
    vid = taro.register_dataset(
        "coco-vehicles",
        base={"manifest_hash": "sha256:abc", "uri": "s3://data/coco"},
        ops=[{"op": "mosaic", "p": 0.5}],
        parent_version_id="ver-base",
        client=client,
    )

    assert vid == "ver-1"
    # Same primitive as config, only the namespace and the {base, ops} shape differ.
    assert client.posts[0] == ("/documents", {"namespace": "dataset", "name": "coco-vehicles"})
    assert client.posts[1] == (
        "/documents/doc-1/versions",
        {
            "body": {
                "base": {"manifest_hash": "sha256:abc", "uri": "s3://data/coco"},
                "ops": [{"op": "mosaic", "p": 0.5}],
            },
            "parent_version_id": "ver-base",
        },
    )
