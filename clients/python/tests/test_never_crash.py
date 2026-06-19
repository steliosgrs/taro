"""Never-crash contract: tracking failures warn and continue.

The SDK's core promise — training must never die because of logging. These run
offline (no server): an unreachable server is simulated by pointing the client
at a dead port.
"""

import logging

import taro
from taro import Run
from taro._client import Client

# Port 1 isn't listening → connection refused → fails fast (no real network).
DEAD_URL = "http://127.0.0.1:1"


def _dead_client():
    return Client(DEAD_URL, timeout=0.5)


def test_start_run_degrades_when_server_down():
    taro.init(DEAD_URL, timeout=0.5)
    run = taro.start_run("exp")
    # Degraded no-op run instead of an exception.
    assert run.ok is False
    assert run.run_id is None


def test_logging_on_degraded_run_is_noop():
    # run_id None → degraded; every log/lifecycle call must be a silent no-op,
    # even though the client points at an unreachable server.
    run = Run(_dead_client(), None)
    run.log_metric("loss", 0.5, step=0)
    run.log_curve("pr", x=[0.0, 1.0], y=[1.0, 0.5], step=0, curve_type="pr")
    run.log_artifact("/no/such/file.pt")
    run.register_artifact("weights", "s3://bucket/best.pt")
    run.link_document("ver-1")  # degraded → no-op, no request attempted
    run.finish()  # no batcher to flush, no PATCH attempted
    assert run.ok is False


def test_register_config_returns_none_when_server_down():
    taro.init(DEAD_URL, timeout=0.5)
    # Soft default + never-crash: a registry outage must not raise; it returns
    # None so the run simply starts un-linked.
    vid = taro.register_config("yolo-baseline", {"lr0": 0.01})
    assert vid is None


def test_log_artifact_missing_file_warns(caplog):
    # An "ok" run (truthy id) but a missing path: open() raises OSError, which
    # log_artifact swallows with a warning rather than crashing the caller.
    run = Run(_dead_client(), "fake-run-id", interval=3600)
    try:
        with caplog.at_level(logging.WARNING, logger="taro"):
            run.log_artifact("/definitely/not/here/best.pt")  # must not raise
        assert "best.pt" in caplog.text
    finally:
        run._batcher.close()  # clean up the idle worker thread
