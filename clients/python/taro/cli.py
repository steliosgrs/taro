"""Taro CLI (M10) — inspect a running server from the terminal.

A thin client over the frozen REST contract, built on `taro._client.Client`. The
surface is read/inspect plus experiment-create; *logging* stays in training code
via the SDK (there is no server-side run-listing endpoint, by design). Unlike the
never-crash SDK, the CLI surfaces errors on stderr and exits non-zero.

    taro health
    taro experiments list
    taro experiments create vehicle-detector
    taro runs get <run_id>
    taro runs metrics <run_id> --key mAP50
    taro runs curves <run_id> --key pr_curve
    taro runs artifacts <run_id>
    taro compare A,B --key pr_curve

Global flags (before the command): --url (env TARO_URL), --api-key
(env TARO_API_KEY), --json (emit the raw server response).
"""

import argparse
import json
import os
import sys
from typing import Any, List, Optional, Sequence

from ._client import Client, TaroHTTPError


# --- output helpers ---------------------------------------------------------

def _emit_json(obj: Any) -> None:
    print(json.dumps(obj, indent=2, default=str))


def _cell(v: Any) -> str:
    return "-" if v is None else str(v)


def _table(rows: Sequence[dict], columns: List[str]) -> None:
    """Print a list of flat dicts as an aligned, header-underlined table."""
    if not rows:
        print("(none)")
        return
    formatted = [{c: _cell(r.get(c)) for c in columns} for r in rows]
    widths = {c: max(len(c), *(len(r[c]) for r in formatted)) for c in columns}
    print("  ".join(c.upper().ljust(widths[c]) for c in columns))
    print("  ".join("-" * widths[c] for c in columns))
    for r in formatted:
        print("  ".join(r[c].ljust(widths[c]) for c in columns))


def _npoints(data: Any) -> int:
    """Number of x-values in a curve `data` payload (0 if absent/odd shape)."""
    if isinstance(data, dict) and isinstance(data.get("x"), list):
        return len(data["x"])
    return 0


# --- command handlers -------------------------------------------------------

def cmd_health(client: Client, args: argparse.Namespace) -> None:
    data = client.health()
    if args.json:
        _emit_json(data)
        return
    print(f"{data.get('status', '?')}  {data.get('service', '')} {data.get('version', '')}".rstrip())


def cmd_exp_list(client: Client, args: argparse.Namespace) -> None:
    data = client.get("/experiments")
    if args.json:
        _emit_json(data)
        return
    _table(data, ["id", "name", "created_at"])


def cmd_exp_create(client: Client, args: argparse.Namespace) -> None:
    exp = client.post("/experiments", {"name": args.name})
    if args.json:
        _emit_json(exp)
        return
    print(f"created experiment {exp['id']}  ({exp['name']})")


def cmd_exp_get(client: Client, args: argparse.Namespace) -> None:
    exp = client.get(f"/experiments/{args.id}")
    if args.json:
        _emit_json(exp)
        return
    for k in ("id", "name", "created_at"):
        print(f"{k:14} {_cell(exp.get(k))}")


def cmd_run_get(client: Client, args: argparse.Namespace) -> None:
    run = client.get(f"/runs/{args.id}")
    if args.json:
        _emit_json(run)
        return
    for k in ("id", "experiment_id", "name", "status", "started_at", "ended_at"):
        print(f"{k:14} {_cell(run.get(k))}")
    for block in ("params", "tags"):
        kv = run.get(block) or {}
        if kv:
            print(f"{block}:")
            for k, v in kv.items():
                print(f"  {k} = {v}")


def cmd_run_metrics(client: Client, args: argparse.Namespace) -> None:
    resp = client.get(f"/runs/{args.id}/metrics", {"key": args.key})
    if args.json:
        _emit_json(resp)
        return
    series = resp.get("series") or {}
    if not series:
        print("(no scalar metrics)")
        return
    for key, points in series.items():
        print(f"{key}  ({len(points)} points)")
        _table(points, ["step", "value", "ts"])
        print()


def cmd_run_curves(client: Client, args: argparse.Namespace) -> None:
    resp = client.get(f"/runs/{args.id}/curves", {"key": args.key, "step": args.step})
    if args.json:
        _emit_json(resp)
        return
    curves = resp.get("curves") or []
    rows = [
        {
            "key": c.get("key"),
            "type": c.get("curve_type"),
            "step": c.get("step"),
            "x_label": c.get("x_label"),
            "y_label": c.get("y_label"),
            "points": _npoints(c.get("data")),
            "ts": c.get("ts"),
        }
        for c in curves
    ]
    _table(rows, ["key", "type", "step", "x_label", "y_label", "points", "ts"])


def cmd_run_artifacts(client: Client, args: argparse.Namespace) -> None:
    arts = client.get(f"/runs/{args.id}/artifacts")
    if args.json:
        _emit_json(arts)
        return
    _table(arts, ["name", "media_type", "size_bytes", "uri", "created_at"])


def cmd_compare(client: Client, args: argparse.Namespace) -> None:
    resp = client.get(
        "/curves/compare",
        {"run_ids": args.run_ids, "key": args.key, "step": args.step or "latest"},
    )
    if args.json:
        _emit_json(resp)
        return
    print(f"key      {resp.get('key')}")
    print(f"x_label  {_cell(resp.get('x_label'))}")
    print(f"y_label  {_cell(resp.get('y_label'))}")
    rows = [
        {
            "run_id": r.get("run_id"),
            "run_name": r.get("run_name"),
            "step": r.get("step"),
            "points": _npoints(r.get("data")),
        }
        for r in resp.get("runs") or []
    ]
    print()
    _table(rows, ["run_id", "run_name", "step", "points"])


# --- parser -----------------------------------------------------------------

def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="taro", description="Taro experiment tracker CLI")
    p.add_argument(
        "--url",
        default=os.environ.get("TARO_URL", "http://localhost:8080"),
        help="server base URL (env TARO_URL)",
    )
    p.add_argument(
        "--api-key",
        default=os.environ.get("TARO_API_KEY"),
        help="bearer token if the server requires auth (env TARO_API_KEY)",
    )
    p.add_argument("--json", action="store_true", help="emit the raw server response as JSON")
    sub = p.add_subparsers(dest="command", required=True)

    sub.add_parser("health", help="server liveness").set_defaults(func=cmd_health)

    exp = sub.add_parser("experiments", help="experiment commands")
    exps = exp.add_subparsers(dest="action", required=True)
    exps.add_parser("list", help="list experiments").set_defaults(func=cmd_exp_list)
    c = exps.add_parser("create", help="get-or-create an experiment")
    c.add_argument("name")
    c.set_defaults(func=cmd_exp_create)
    g = exps.add_parser("get", help="experiment detail")
    g.add_argument("id")
    g.set_defaults(func=cmd_exp_get)

    runs = sub.add_parser("runs", help="run commands")
    rs = runs.add_subparsers(dest="action", required=True)
    rg = rs.add_parser("get", help="run detail (params + tags)")
    rg.add_argument("id")
    rg.set_defaults(func=cmd_run_get)
    rm = rs.add_parser("metrics", help="scalar series")
    rm.add_argument("id")
    rm.add_argument("--key", help="filter to one metric key")
    rm.set_defaults(func=cmd_run_metrics)
    rc = rs.add_parser("curves", help="curve metrics")
    rc.add_argument("id")
    rc.add_argument("--key", help="filter to one curve key")
    rc.add_argument("--step", help="step number, or 'latest'")
    rc.set_defaults(func=cmd_run_curves)
    ra = rs.add_parser("artifacts", help="logged artifacts")
    ra.add_argument("id")
    ra.set_defaults(func=cmd_run_artifacts)

    cmp = sub.add_parser("compare", help="overlay N runs' curves for one key")
    cmp.add_argument("run_ids", help="comma-separated run ids, e.g. A,B")
    cmp.add_argument("--key", required=True, help="curve key to overlay")
    cmp.add_argument("--step", help="step number, or 'latest' (default)")
    cmp.set_defaults(func=cmd_compare)

    return p


def main(argv: Optional[Sequence[str]] = None) -> None:
    args = build_parser().parse_args(argv)
    client = Client(args.url, api_key=args.api_key)
    try:
        args.func(client, args)
    except TaroHTTPError as e:
        print(f"taro: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
