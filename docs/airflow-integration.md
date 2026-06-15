# Taro — Airflow Integration (FUTURE / NOT IN POC)

> **Status: out of scope for the POC.** This document captures the intended design,
> its caveats, and the decisions to settle *before* building it. Nothing here is
> implemented or required for the POC. The POC ships Taro as a standalone tracker
> (server + REST API + Python SDK + CLI) with no orchestration.

---

## 1. Purpose & boundary

Separate the two concerns cleanly:

- **Airflow = orchestration** — *when/where* a job runs: scheduling, queueing, concurrency, retries, dependencies.
- **Taro = tracking** — *what a run produced*: params/config, scalar metrics, curve metrics, artifacts, status.

They are complementary layers (the standard MLOps split, e.g. Airflow + MLflow). Taro must remain a tracker; it must **not** become a scheduler.

**Hard rule:** the **Taro *server* never orchestrates.** A developer, a CLI, or a thin outbox may trigger Airflow; the Taro server's job is to store config and receive results.

---

## 2. Target developer experience

The developer interacts with **Taro only** — they choose an experiment config and submit it; they never open Airflow. Submission is **asynchronous / fire-and-forget**: submit → walk away → watch progress and results in Taro. Airflow is invisible background plumbing.

Run lifecycle the developer observes (entirely in Taro):

```
submitted → running → finished | failed   (cancelled — future)
```

"Airflow task finished" and "Taro run finished" are the same event seen from two sides; the developer only ever looks at the Taro side.

---

## 3. Options considered

| Option | Dev touches Airflow? | Taro stays pure tracker? | Who owns the queue? | Verdict |
|---|---|---|---|---|
| **A — push (CLI triggers Airflow)** | a little (one CLI cmd) | ✅ yes | Airflow | OK, but dev leaves Taro to trigger |
| **B — pull (Airflow polls a Taro registry)** | never | ❌ Taro becomes a job queue | Taro (you build claim/heartbeat/cancel) | Rejected — reinvents a queue inside the tracker |
| **Hybrid — submit to Taro → Taro pushes trigger** | never | ⚠️ tracker + thin "config inbox" | Airflow | **Recommended when this is built** |

Model B is explicitly **not** recommended: it drags concurrency control, atomic claiming, orphan reaping, and cancellation into Taro — the exact orchestration weight we want to avoid.

---

## 4. Recommended design (Hybrid) — for when this is built

```mermaid
sequenceDiagram
    participant Dev as Developer
    participant Taro as Taro (tracker + config inbox)
    participant OB as Trigger outbox
    participant AF as Airflow (owns queue/retries)
    participant Op as TaroExperimentOperator
    participant Train as Training job

    Dev->>Taro: submit config (CLI/API)
    Taro->>Taro: validate config, create run (status=submitted)
    Taro->>OB: write pending-trigger (SAME DB txn)
    Taro-->>Dev: run_id  (walks away)

    OB->>AF: POST dagRun (dag_run_id = run_id, conf=config)
    Note over AF: Airflow owns scheduling,<br/>pools (concurrency), retries
    AF->>Op: execute task
    Op->>Taro: status → running
    Op->>Train: launch(run_id, config)
    loop epochs
        Train->>Taro: log_metric / log_curve (live)
    end
    Train->>Taro: log_artifact ; finish
    Op->>Taro: status → finished / failed
    Dev->>Taro: run show / watch curves (anytime)
```

### Component responsibilities
- **Taro server** — validate config, create run, store config as params, emit *one* trigger via the outbox, receive all metrics/curves/artifacts/status. A thin "config inbox," not a queue.
- **Trigger outbox** — a `pending_trigger` row written in the *same DB transaction* as the run; drained by a small retrier that calls the Airflow REST API. Makes the single Taro→Airflow handoff reliable.
- **Airflow** — the real engine: queue, concurrency (pools), retries, scheduling.
- **`TaroExperimentOperator`** — glue only: flip Taro status → running, launch training with `run_id`+config, report terminal status; reuses the existing Python SDK.

### New Taro surface this would require (none of it in POC)
- A `submit` endpoint + config validation.
- A `status` field extended with `submitted` (pre-`running`).
- A `pending_trigger` outbox table + retrier.
- Airflow API credentials + network reachability + a deployed DAG.

---

## 5. Caveats & pain points (why this is deferred)

1. **Dual-write problem (top risk).** "Create run" + "tell Airflow" touch two systems. Naive two-call code yields ghosts: run created but trigger lost (stuck `submitted`), or triggered but DB rolled back (orphan DAG run). The **outbox is mandatory**, not optional — never trigger synchronously inside the request handler.
2. **Idempotency required.** The outbox retries; without `dag_run_id = run_id`, retries spawn duplicate DAG runs. Setting it lets Airflow dedupe for free.
3. **Retry semantics are ambiguous.** Airflow retries a failed task — does attempt #2 reuse the same Taro run (metrics collide on `step`, partial curves mix) or create a new linked run? Unresolved (see decisions).
4. **Cancellation only half-solved.** Marking a run `cancelled` in Taro does not kill a *running* Airflow task without Taro→Airflow signalling (re-touches the boundary). Cancelling *queued/future* work is clean; killing in-flight is not.
5. **Light reaping still needed.** If the operator crashes before writing terminal status, Taro stays `running`. Airflow `on_failure_callback` covers most cases (better than pure B), but a heartbeat/timeout sweeper is still required.
6. **New coupling surface.** Taro gains a dependency on Airflow (creds, reachability, an existing DAG) — real ops weight and a security surface.
7. **Two sources of truth.** Airflow task state vs. Taro run state can drift; needs reconciliation.
8. **Residual orchestration creep.** Even the hybrid pulls a little orchestration weight (outbox + reaping) into Taro — far less than pure B, but non-zero. This is the price of the pure-Taro developer experience.

---

## 6. Open decisions (settle before building)

- [ ] **D1 — Trigger reliability:** confirm transactional outbox + retrier (recommended) vs. synchronous trigger (rejected as fragile).
- [ ] **D2 — Idempotency key:** confirm `dag_run_id = run_id`.
- [ ] **D3 — Retry model:** Airflow retry = **new child run** (recommended; no step collisions) vs. reuse same run. Decide parent/child run linkage in the data model.
- [ ] **D4 — Cancellation policy:** POC-of-this-feature stance = cancel stops queued/future only; in-flight kill via operator polling a Taro cancel flag is later/optional.
- [ ] **D5 — Reaping:** heartbeat field + sweeper timeout values; who writes `failed` on operator crash (Airflow callback vs. sweeper).
- [ ] **D6 — Config schema & validation:** define the experiment config contract; validate at submit so bad configs never reach Airflow.
- [ ] **D7 — Concurrency authority:** Airflow pools own max-parallel (GPU limits); Taro does **not** cap concurrency (avoid double-capping).
- [ ] **D8 — Auth/secrets:** how Taro stores Airflow API creds; network path Taro→Airflow.
- [ ] **D9 — DAG shape:** one generic "run experiment" DAG parameterised by conf vs. per-experiment DAGs.

---

## 7. Relationship to the POC

The POC is designed so this integration is **additive, not a rewrite**:
- The `TaroExperimentOperator` is just another consumer of the **existing Python SDK** — no special server path.
- Config-as-params already exists in the run model; `submit`/`submitted`/outbox are the only new server pieces, and they bolt on without touching the metric/curve core.

Nothing in the POC should assume Airflow exists. Build and validate the standalone tracker first; revisit this document only once the POC proves the curve-native tracking value.
