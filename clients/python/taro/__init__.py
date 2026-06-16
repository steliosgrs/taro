"""Taro Python SDK (M4) — ergonomic, framework-agnostic logging over the REST API.

    import taro
    taro.init("http://localhost:8080")
    with taro.start_run("yolo-vehicle-detector", params={"lr0": 0.01}) as run:
        run.log_metric("mAP50", 0.64, step=50)
        run.log_curve("pr_curve", x=recall, y=precision, step=50,
                      curve_type="pr", x_label="recall", y_label="precision")

The core imports no ML frameworks; adapters (`taro.integrations.*`) come later and
only call these methods. Artifacts arrive in M5.
"""

import logging
from typing import Optional, Sequence

from ._client import Client, TaroHTTPError
from .run import Run

__all__ = ["init", "start_run", "compare_curves", "Run", "Client", "TaroHTTPError"]

log = logging.getLogger("taro")

_default: Optional[Client] = None


def init(
    base_url: str = "http://localhost:8080",
    api_key: Optional[str] = None,
    timeout: float = 10.0,
) -> Client:
    """Configure the process-wide default client used by `start_run`."""
    global _default
    _default = Client(base_url, api_key=api_key, timeout=timeout)
    return _default


def _require_client(client: Optional[Client]) -> Client:
    c = client or _default
    if c is None:
        raise RuntimeError("call taro.init(base_url) first (or pass client=)")
    return c


def start_run(
    experiment: str,
    name: Optional[str] = None,
    params: Optional[dict] = None,
    tags: Optional[dict] = None,
    *,
    client: Optional[Client] = None,
    batch_size: int = 100,
    interval: float = 5.0,
) -> Run:
    """Start (and get-or-create the experiment for) a run.

    Never-crash: if the server is unreachable, returns a degraded no-op `Run`
    (`run.ok is False`) instead of raising, so training proceeds untracked.
    """
    c = _require_client(client)
    body = {
        "experiment": experiment,
        "name": name,
        "params": params or {},
        "tags": tags or {},
    }
    try:
        resp = c.post("/runs", body)
        return Run(
            c, resp["run_id"], resp.get("experiment_id"),
            batch_size=batch_size, interval=interval,
        )
    except (TaroHTTPError, KeyError) as e:
        log.warning("taro: could not start run for '%s' (%s); logging disabled", experiment, e)
        return Run(c, None)


def compare_curves(
    run_ids: Sequence[str],
    key: str,
    step: str = "latest",
    *,
    client: Optional[Client] = None,
) -> dict:
    """Overlay N runs' curves for one key — the reason Taro exists. Returns
    comparable curve *data*, never an image."""
    c = _require_client(client)
    ids = run_ids if isinstance(run_ids, str) else ",".join(run_ids)
    return c.get("/curves/compare", {"run_ids": ids, "key": key, "step": step})
