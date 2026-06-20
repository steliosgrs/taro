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
from typing import Any, Optional, Sequence

from ._client import Client, TaroHTTPError
from .run import Run

__all__ = [
    "init",
    "start_run",
    "register_config",
    "register_dataset",
    "compare_curves",
    "Run",
    "Client",
    "TaroHTTPError",
]

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


def _register_document(
    name: str,
    body: dict,
    *,
    namespace: str,
    parent_version_id: Optional[str],
    client: Optional[Client],
    kind: str,
) -> Optional[str]:
    """Get-or-create the `(namespace, name)` handle and publish `body` as a new,
    content-addressed version; returns the version id. Shared by the typed
    `register_config` / `register_dataset` helpers — never-crash (warns and
    returns `None` on any failure, so the run is simply un-linked)."""
    c = _require_client(client)
    try:
        doc = c.post("/documents", {"namespace": namespace, "name": name})
        payload: dict = {"body": body}
        if parent_version_id is not None:
            payload["parent_version_id"] = parent_version_id
        version = c.post(f"/documents/{doc['id']}/versions", payload)
        return version["version_id"]
    except (TaroHTTPError, KeyError) as e:
        log.warning("taro: could not register %s '%s' (%s); continuing without it", kind, name, e)
        return None


def register_config(
    name: str,
    body: dict,
    *,
    namespace: str = "config",
    parent_version_id: Optional[str] = None,
    client: Optional[Client] = None,
) -> Optional[str]:
    """Register a config in the versioned-document registry and return its
    version id (pass it to `start_run(config_version_id=...)`).

    Get-or-creates the `(namespace, name)` handle and publishes `body` as a new,
    content-addressed version — re-registering identical content is idempotent
    (the server returns the existing version, no duplicate). Soft default: this is
    optional. Never-crash — on any failure it warns and returns `None`, so a run
    started with that `None` is simply un-linked rather than blocked.
    """
    return _register_document(
        name, body, namespace=namespace,
        parent_version_id=parent_version_id, client=client, kind="config",
    )


def register_dataset(
    name: str,
    base: dict,
    ops: Optional[Sequence[dict]] = None,
    *,
    namespace: str = "dataset",
    parent_version_id: Optional[str] = None,
    client: Optional[Client] = None,
) -> Optional[str]:
    """Register a dataset *recipe* (registry Slice 2) and return its version id;
    link it to a run with `run.link_document(version_id, role="dataset")`.

    A recipe is the declarative body `{"base": base, "ops": [...]}`: `base`
    identifies the source data (e.g. `{"manifest_hash": ..., "uri": ...}`) and
    `ops` is the ordered list of transforms applied to it. Pass `parent_version_id`
    to record a variation-of-a-variation (the lineage DAG). The server stores the
    recipe as opaque data and **never executes it** — applying the recipe is an
    adapter's job; a recipe captures intent + pinned base/seed, not bit-exact
    bytes. Same soft-default, never-crash posture as `register_config`.
    """
    body = {"base": base, "ops": list(ops or [])}
    return _register_document(
        name, body, namespace=namespace,
        parent_version_id=parent_version_id, client=client, kind="dataset",
    )


def start_run(
    experiment: str,
    name: Optional[str] = None,
    params: Optional[dict] = None,
    tags: Optional[dict] = None,
    *,
    config_version_id: Optional[str] = None,
    client: Optional[Client] = None,
    batch_size: int = 100,
    interval: float = 5.0,
) -> Run:
    """Start (and get-or-create the experiment for) a run.

    `config_version_id` (from `register_config`) links the run to a registered
    config — the run's structured source of record, separate from free `params`.

    Never-crash: if the server is unreachable, returns a degraded no-op `Run`
    (`run.ok is False`) instead of raising, so training proceeds untracked.
    """
    c = _require_client(client)
    body: dict[str, Any] = {
        "experiment": experiment,
        "name": name,
        "params": params or {},
        "tags": tags or {},
    }
    if config_version_id is not None:
        body["config_version_id"] = config_version_id
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
