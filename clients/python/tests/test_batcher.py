"""Unit tests for ScalarBatcher (working; no server needed)."""

import threading
import time

from taro._batch import ScalarBatcher


def _collector():
    """A flush_fn that records batches and signals when one arrives."""
    received = []
    got = threading.Event()

    def flush_fn(batch):
        received.extend(batch)
        got.set()

    return received, got, flush_fn


def _point(step):
    return {"key": "loss", "step": step, "value": float(step)}


def test_flush_on_size():
    received, got, flush_fn = _collector()
    b = ScalarBatcher(flush_fn, batch_size=2, interval=60)
    b.add(_point(0))
    b.add(_point(1))  # hits batch_size → early flush
    assert got.wait(2.0), "batcher did not flush on size"
    b.close()
    assert len(received) == 2


def test_flush_on_interval():
    received, got, flush_fn = _collector()
    b = ScalarBatcher(flush_fn, batch_size=100, interval=0.1)
    b.add(_point(0))  # below size; only the timer can flush it
    assert got.wait(2.0), "batcher did not flush on interval"
    b.close()
    assert len(received) >= 1


def test_close_drains_remainder():
    received, _got, flush_fn = _collector()
    b = ScalarBatcher(flush_fn, batch_size=100, interval=60)  # never hits size/timer
    b.add(_point(0))
    b.close()  # must drain the leftover
    assert len(received) == 1


def test_flush_error_is_swallowed():
    def flush_fn(_batch):
        raise RuntimeError("boom")  # simulate server/network failure

    b = ScalarBatcher(flush_fn, batch_size=1, interval=60)
    b.add(_point(0))  # triggers a flush that raises internally
    time.sleep(0.2)
    b.close()  # neither add nor close may propagate the error
