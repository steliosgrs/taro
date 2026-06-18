"""Context-manager lifecycle.

The `with taro.start_run(...)` block must finalize the run on exit — "finished"
on a clean exit, "failed" on an exception — and must NOT swallow a training
exception (only tracking errors are swallowed). Driven with a recording client
so no server is needed.
"""

import pytest

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
