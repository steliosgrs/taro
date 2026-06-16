"""The `Run` handle: a context manager that auto-finalizes and logs metrics.

A run whose creation failed is kept as a *degraded* no-op (`run_id is None`):
every log call quietly does nothing so a tracking outage never crashes training.
"""

import logging
from typing import Any, Dict, Optional, Sequence

from ._batch import ScalarBatcher
from ._client import Client, TaroHTTPError

log = logging.getLogger("taro")


class Run:
    def __init__(
        self,
        client: Client,
        run_id: Optional[str],
        experiment_id: Optional[str] = None,
        *,
        batch_size: int = 100,
        interval: float = 5.0,
    ):
        self.client = client
        self.run_id = run_id
        self.experiment_id = experiment_id
        self._batcher = (
            ScalarBatcher(self._send_scalars, batch_size, interval) if run_id else None
        )

    @property
    def ok(self) -> bool:
        """False for a degraded run (creation failed); all logging is a no-op."""
        return self.run_id is not None

    # ----- logging ------------------------------------------------------------
    def _send_scalars(self, points) -> None:
        self.client.post(f"/runs/{self.run_id}/metrics", {"metrics": points})

    def log_metric(self, key: str, value: float, step: int = 0) -> None:
        if not self.ok:
            return
        self._batcher.add({"key": key, "value": float(value), "step": int(step)})

    def log_curve(
        self,
        key: str,
        x: Sequence[float],
        y: Optional[Sequence[float]] = None,
        *,
        series: Optional[Sequence[Dict[str, Any]]] = None,
        step: int = 0,
        curve_type: str = "generic_xy",
        x_label: Optional[str] = None,
        y_label: Optional[str] = None,
        labels: Optional[Sequence[str]] = None,
    ) -> None:
        """Log one curve (flushed immediately). Provide exactly one of `y` or
        `series` (a list of `{"name", "y"}`)."""
        if not self.ok:
            return
        data: Dict[str, Any] = {"x": list(x)}
        if series is not None:
            data["series"] = [{"name": s["name"], "y": list(s["y"])} for s in series]
        if y is not None:
            data["y"] = list(y)
        if labels is not None:
            data["labels"] = list(labels)
        curve = {
            "key": key,
            "step": int(step),
            "curve_type": curve_type,
            "x_label": x_label,
            "y_label": y_label,
            "data": data,
        }
        try:
            self.client.post(f"/runs/{self.run_id}/curves", {"curves": [curve]})
        except TaroHTTPError as e:  # never-crash
            log.warning("taro: failed to log curve '%s' (%s)", key, e)

    def log_artifact(
        self,
        path: str,
        name: Optional[str] = None,
        media_type: Optional[str] = None,
    ) -> None:
        """Upload a local file's bytes to the run's blob store (M5)."""
        if not self.ok:
            return
        try:
            self.client.post_file(
                f"/runs/{self.run_id}/artifacts", path, name=name, media_type=media_type
            )
        except (TaroHTTPError, OSError) as e:  # never-crash (incl. missing file)
            log.warning("taro: failed to log artifact '%s' (%s)", path, e)

    def register_artifact(
        self,
        name: str,
        uri: str,
        media_type: Optional[str] = None,
        size_bytes: Optional[int] = None,
    ) -> None:
        """Record an artifact that already lives at a URI (e.g. `s3://…`)."""
        if not self.ok:
            return
        body = {"name": name, "uri": uri, "media_type": media_type, "size_bytes": size_bytes}
        try:
            self.client.post(f"/runs/{self.run_id}/artifacts", body)
        except TaroHTTPError as e:  # never-crash
            log.warning("taro: failed to register artifact '%s' (%s)", name, e)

    # ----- lifecycle ----------------------------------------------------------
    def finish(self, status: str = "finished") -> None:
        if not self.ok:
            return
        if self._batcher:
            self._batcher.close()  # flush buffered scalars before finalizing
        try:
            self.client.patch(f"/runs/{self.run_id}", {"status": status})
        except TaroHTTPError as e:  # never-crash
            log.warning("taro: failed to finalize run %s (%s)", self.run_id, e)

    def __enter__(self) -> "Run":
        return self

    def __exit__(self, exc_type, exc, tb) -> bool:
        # A training exception finalizes the run as 'failed' but is NOT swallowed.
        self.finish("failed" if exc_type else "finished")
        return False
