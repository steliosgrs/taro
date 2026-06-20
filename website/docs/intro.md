---
sidebar_position: 1
---

# Introduction

**Taro** is a self-hosted, **curve-native** experiment tracker — an MLflow
alternative whose load-bearing idea is that a metric value can be a
**curve/vector**, stored as structured data so N runs' PR curves can be
overlaid (the thing MLflow can't do).

:::note Scaffold

This docs site is set up (tooling, theme, build) but the **content structure is
yours to shape**. Add Markdown/MDX files under `docs/` and they appear in the
sidebar automatically. Suggested sections to fill in: Quickstart, REST API,
Python SDK, CLI, Concepts (curves, the document registry), Deployment (Docker).

The canonical design notes already live in the repo under `docs/` (e.g.
`poc-design.md`) — decide whether to import them here or link out.

:::

## Run it

```bash
docker compose --profile seed up --build   # server + Postgres + demo seed
curl localhost:8080/health
```
