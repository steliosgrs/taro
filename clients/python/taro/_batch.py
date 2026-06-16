"""Background scalar batcher.

Scalars are high-frequency (per-step), so they are buffered and flushed on size
or time rather than one HTTP call per point. Curves/artifacts are low-frequency
and flush immediately, so they don't go through here.

Never-crash: a flush failure logs a warning and drops the batch; it never raises
into the training loop.
"""

import logging
import threading
from typing import Any, Callable, Dict, List

log = logging.getLogger("taro")

Point = Dict[str, Any]


class ScalarBatcher:
    def __init__(
        self,
        flush_fn: Callable[[List[Point]], None],
        batch_size: int = 100,
        interval: float = 5.0,
    ):
        self._flush_fn = flush_fn
        self._batch_size = batch_size
        self._interval = interval
        self._buf: List[Point] = []
        self._lock = threading.Lock()
        self._wake = threading.Event()   # set to flush early (buffer full / closing)
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._loop, name="taro-batcher", daemon=True)
        self._thread.start()

    def add(self, point: Point) -> None:
        with self._lock:
            self._buf.append(point)
            full = len(self._buf) >= self._batch_size
        if full:
            self._wake.set()

    def _drain(self) -> List[Point]:
        with self._lock:
            batch, self._buf = self._buf, []
        return batch

    def flush(self) -> None:
        batch = self._drain()
        if not batch:
            return
        try:
            self._flush_fn(batch)
        except Exception as e:  # never-crash: tracking must not kill training
            log.warning("taro: dropped %d scalar point(s) (%s)", len(batch), e)

    def _loop(self) -> None:
        while not self._stop.is_set():
            self._wake.wait(self._interval)   # wake on timeout OR early signal
            self._wake.clear()
            self.flush()

    def close(self) -> None:
        """Stop the worker and flush whatever remains (called on run finish)."""
        self._stop.set()
        self._wake.set()
        self._thread.join(timeout=self._interval + 5)
        self.flush()
