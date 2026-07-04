---
scope: L2
summary: "Lifecycle/resource robustness audit — re-verified debt register + 22 new rows (N1-N22) + watchdog census + top-10. Evidence base for the lifecycle-and-errors RFC (next wave after M9). Read-only audit; paths current as of M8 tip, M9 in flight."
modified: 2026-07-02
reviewed: 2026-07-02
lifecycle: ephemeral
type: AUDIT
state: evidence-complete
depends:
  - path: issues/L2-runtime-lifecycle-debt
dependents: []
---

All five deep-audits are in. Here is the consolidated lifecycle & resource-robustness register.

---

# L2 Lifecycle/Resource Robustness Audit — re-verified register + new rows

> **Status: EVIDENCE/AUDIT (evidence-complete).** Read-only investigation, not a
> plan. This is the N1–N22 + top-10 evidence base for
> RFC-L2-lifecycle-and-errors, whose M20–M31 fixes shipped. Paths are current as
> of the M8 tip (M9 in flight at audit time). Findings preserved as provenance.

Scope honored the writer-agent exclusions (`link-far-end`, `far_end/`, `transport.rs`, `read.rs`, `link_wire`, `runtime_registry`, `local_authority_server` were not deep-audited; where a finding touches their *ownership seam* it is flagged). Paths are relative to `/home/usr.prj_posthaste/src/.workspaces/architecture-cleanup`. The register's old paths (`build.rs:281`, `supervisor/manager.rs:95`, `authority-runtime/push.rs`) moved in the M0–M8 refactor; re-verified against current code.

## Part A — Re-verification of the existing register

| # | Status | Current evidence |
|---|--------|------------------|
| 1 | **closed-by-M9** (skipped) | `transport.rs` — writer zone |
| 2 | **closed-by-M9** (skipped) | down-channel now spawned at `assembly.rs:276` (was `build.rs:281`); reconciler is M9 |
| 3 | **STILL OPEN, extended** | `shutdown.rs:19-31` — `RuntimeShutdownHandle::shutdown` only does `stopped.store(true)`; no task-join, no store flush, no supervisor stop. The one reader of `stopped` is `handle.rs:127` (labels lifecycle "Stopped" in status reads). Supervisor still hard-aborts: `supervisor/manager.rs:98` `runtime.handle.abort()`. **Extension:** the handle is *never invoked in any production path* — only `authority_server_handle.rs` tests + `posthaste-server/src/tests.rs:90` call `.shutdown()`. See N1–N3. |
| 4 | **STILL OPEN** | `push.rs:153-154` jitterless `current_delay = (current_delay*2).min(max)`; `loop` never exits (unbounded retry); fallback→primary cycle resets `consecutive_failures = 0` (`push.rs:137-138`) → infinite primary↔fallback cycling; no poll-only degraded tier. |
| 5 | **STILL OPEN** | `supervisor/runtime.rs:67-121` — every `select!` arm inline-`.await`s (`process_sync_trigger_with_state`, backfill, cache, snooze, oauth, push); a hung provider sync at `sync_flow.rs:84-107` blocks push/commands/ticks for that account. No `tokio::time::timeout` anywhere in the crate. |
| 6 | **not re-verified** — `posthaste-imap` outside audited crates (store-side equivalent captured in N15) |
| 7 | **not re-verified** — `posthaste-domain-service/outbox.rs` outside audited crates. NB: `posthaste-store/src/outbox.rs` is a *different* outbox (persistence); its own gaps in N15. |
| 8 | **closed-by-M9**, confirmed present | `sessionClient.ts:50-55` flat `setTimeout(…,1000)` |
| 9 | **closed-by-M9**, confirmed present | `httpAdapter.ts:282,344`, `core.ts:219` unchecked `as` casts |
| 10 | **STILL OPEN** | `supervisor/runtime.rs:241-244` — `handle_snooze_tick` uses `SystemTime::now().duration_since(UNIX_EPOCH)` (wall-clock) for due comparison. Unchanged. |

## Part B — NEW rows (symptom · evidence · failure scenario · principle)

**Shutdown / drain**

- **N1 — No process shutdown sequence at all; no signal handler.** `posthaste-server/src/main.rs:70-73`, `bin/authority_server.rs:37`, `runtimed/main.rs:77-80` each just `handle.join_handle.await`. `serve.rs:229-243` calls `axum::serve(...)` with **no `.with_graceful_shutdown`**. Repo-wide there is no `ctrl_c`/SIGTERM/SIGINT handler. *Failure:* SIGTERM kills via OS default — in-flight requests/SSE cut mid-write, no runtime shutdown, no store close, no drain deadline. *(Principle XII.)*
- **N2 — No supervisor-level shutdown; account tasks detached, only ever hard-aborted.** `AccountSupervisor` has no `stop_all`/`shutdown`/`impl Drop`; tasks spawned at `supervisor/manager.rs:72` are stored in a `HashMap` (`types.rs:149`) and only `.abort()`ed per-account (`manager.rs:98`), never `.await`ed. `build.rs:114` keeps it in an `Arc` "for the node's life." *Failure:* on drop the `JoinHandle`s detach — account tasks keep running (or on process kill, die mid-sync); a partially-applied sync/outbox flush is left to next-startup recovery. *(Principle XII.)*
- **N3 — Store never closed/checkpointed.** No `close`/`flush`/`wal_checkpoint`/store-level `Drop` in `posthaste-store` (only `store.rs:51-61` returns a read conn to the pool). WAL + `synchronous=NORMAL` (`db/connection.rs:18,21`). *Failure:* combined with N1, the process is signal-killed with the WAL uncheckpointed → WAL grows across restarts; power-loss loses committed-but-unfsynced txns. *(Principle XII.)*
- **N7 — Down-channel bridge task untracked for shutdown.** `assembly.rs:276` `tokio::spawn(run_authority_server_down_channel(...))` — `JoinHandle` dropped; the `stopped` flag is not wired into it, so `shutdown()` cannot stop it. *(Ownership seam only; `read.rs` internals are M9 zone.)* *(Principle XII.)*

**Deadline census (beyond M9's transport rows)**

- **N4 — Blocking rusqlite I/O runs on tokio worker threads; zero `spawn_blocking`.** Confirmed no `spawn_blocking`/`block_in_place` in any backend crate. `store.rs:196-208` holds the write `Mutex` across the *entire* operation closure (e.g. a full `apply_sync_batch`); `busy_timeout(5s)` (`connection.rs:33`). *Failure:* a contended or large write blocks a tokio worker up to 5 s, starving the async runtime; the write Mutex serializes all writers for the batch duration. *(Principle V/VI.)*
- **N10 — No request/handler timeouts; no `TimeoutLayer`.** `serve.rs:194` applies only Trace+Cors. HTTP-handler awaits are all unbounded: `runtime_stream/sessions.rs:85-93` (`subscribe_runtime_frames`), `sync_events.rs:120-124` (`subscribe_events`), `mutations.rs:46-51` (`forward_mutation`), `views.rs:28-113`. *Failure:* a wedged runtime hangs the handler indefinitely. Only deadline in the whole adapter is the TLS handshake (`tls.rs:34,112`). *(Principle VI.)*
- **N11 — OAuth: no HTTP timeout on any IdP call.** Single client build `oauth/service.rs:20-26` — `reqwest::ClientBuilder…build()` with **no `.timeout()`/`.connect_timeout()`**; re-created per request at `oauth_routes/handlers.rs:38,113,195` and `supervisor/connection.rs:284`. Un-timed calls: token exchange `service.rs:75-80`, refresh `service.rs:135-139`, OIDC discovery `jwks.rs:33-38`, JWKS `jwks.rs:49-54`. *Failure:* a hung IdP wedges the `/v1/oauth/callback` handler and the supervisor's token-refresh path (which feeds row 5's inline loop). *(Principle VI.)*
- **N13 — JWKS cache stampede + hard-fail.** `oauth.rs:26` cache; `jwks.rs:12` releases the read lock before `fetch_jwks` (`jwks.rs:20`) with no single-flight. *Failure:* N concurrent validations with a cold/expired cache each fetch metadata+JWKS (thundering herd); any fetch error propagates and fails the whole code exchange (no stale-cache fallback), inheriting N11's missing timeout. *(Principle VI/VII.)*

**Growth bounds / hostile-input allocation**

- **N12 — OAuth `flow_store` unbounded under callback-less flood.** `oauth/flow_store.rs:42` `Mutex<HashMap>`; TTL prune `flow_store.rs:97-106` invoked from *one* site — `begin_completion` (`flow_store.rs:71`); `insert` (called by unauth `/oauth/start`, `handlers.rs:44`) does not prune; no background sweep. *Failure:* repeated `/oauth/start` with no callback grows a map of secrets-bearing `PendingOAuthFlow` (`flow_store.rs:15-24`) unbounded — unauth memory DoS. *(Principle IV/XIX.)*
- **N15 — LIMIT-less store reads + startup full-table scan under the write lock.** `outbox.rs:172-212` (`list_flushable/pending/unsettled`) and `snooze.rs:126-141` (`list_due_snoozes`) collect all matching rows into a `Vec` with no `LIMIT`; `cache/body_objects.rs:135-169` runs three correlated `DELETE … NOT EXISTS` scans inside `init_schema` on every open (`db/schema.rs:40`), holding the write lock. *Failure:* a large stuck outbox / mass-snooze drives unbounded memory + query time; startup repair is an unbounded scan blocking first writes. *(Principle IV.)*
- **N16 — Read-connection peak unbounded.** `store.rs:172-190` opens a fresh `Connection` when the idle pool is empty; the cap (`MAX_IDLE_READ_CONNECTIONS = 4`, `store.rs:8`) bounds only *retained* conns. *Failure:* N concurrent readers → N simultaneous SQLite connections/file handles, uncapped. *(Resource budget.)*

**Spawn census**

- **N5 — Unbounded detached spawns for sync progress.** `supervisor/sync_flow.rs:19-27` — each `SyncProgressReporter` callback does `tokio::spawn(set_sync_progress(...))` with no retained handle and no bound; concurrent tasks race, so a later progress value can be overwritten by an earlier one landing after it. *Failure:* a chatty sync spawns many orphaned tasks; on shutdown they vanish. *(Spawn budget/ownership.)*
- **N6 — Unbounded TLS handshake `JoinSet`.** `tls.rs:87,111` `handshakes.spawn(...)` per accepted TCP conn, no concurrency cap; each is 10 s-bounded (`tls.rs:34,112`) but reaped only while axum polls `accept` (`tls.rs:139`). *Failure:* a connection flood grows the JoinSet with up-to-10 s-lived tasks. *(Principle IV / spawn budget.)*

**Drain / backpressure**

- **N8 — Runtime event stream silently drops on broadcast `Lagged`.** `handle.rs:263` `Err(RecvError::Lagged(_)) => {}` (channel cap 512, `assembly.rs:25`). The `/v1/events` SSE stream (`sync_events.rs:110-128`) consumes this; `afterSeq` replay only covers reconnect, not mid-stream lag. Contrast the *session* frame stream, which recovers via re-snapshot. *Failure:* a slow `/v1/events` client silently misses events → silent staleness with no gap marker. *(Drain/backpressure; VIII — no resync.)*

**Resource release / liveness**

- **N9 — SSE session/view server-side leak; no idle reaper.** On SSE disconnect axum drops the `broadcast::Receiver`, but the runtime session and its open views persist in the registry until an explicit `DELETE` (`runtime_stream/sessions.rs:47`, `views.rs:61`); no idle-session reaper exists. *Failure:* open→stream→disconnect-without-DELETE leaks sessions + views indefinitely. *(Resource release.)*
- **N14 — P6-style orphaned `.eml` on transaction rollback.** `store.rs:230-231` writes the content-addressed body file *before* the write txn; same in `mutations/commands.rs:114-116` and `mutations/sync_batch.rs:19` (bodies staged before `write_transaction` at `commands.rs:54`). *Failure:* if the txn rolls back, the `.eml` remains with no DB row; reclaimed only if an identical body is later re-fetched. No compensating cleanup. *(Resource release / P6.)*
- **N17 — OAuth refresh + connection rebuild also inline in the supervisor loop.** `supervisor/runtime.rs:398-445` `handle_oauth_refresh_tick` inline-awaits `resolve_secret()` and `ensure_connection()`; a hung secret resolver or gateway rebuild blocks the account's whole `select!` loop. Extends row 5. *(Principle V.)*
- **Open zombie (minor):** `posthaste-server/src/main.rs:184-197` `open`/`xdg-open` `.spawn()` — `Child` dropped without `.wait()`; `<defunct>` until daemon exit.
- **Poison recovery is silent:** `store.rs:63-81` recovers a poisoned write/read Mutex via `into_inner()` (warn-only).

**apps/web (page lifecycle / IndexedDB / worker — new dimensions)**

- **N18 — No unload lifecycle; durability write unawaited; SSE kept open while hidden.** No `beforeunload`/`pagehide`/`visibilitychange` anywhere. Outbox `put` durability (`replica/outboxStore.ts:72-78`) isn't gated on unload; SSE uses `openWhenHidden:true` (`httpAdapter.ts:263,325`) and teardown is React-unmount only (`sessionClient.ts:160-176`). *Failure:* tab close between `acceptMutation` and IDB `oncomplete` loses the intent; backgrounded tabs pin SSE connections; server session never proactively closed. *(Lifecycle/durability.)*
- **N19 — IndexedDB multi-tab upgrade deadlock.** `replica/replicaDatabase.ts:32-57` sets no `onblocked` on `open` and no `connection.onversionchange`. *Failure:* Tab A (v2) blocks Tab B's (v3) upgrade; B's open promise hangs forever, A never closes-on-demand → views stuck loading on deploy. *(Liveness.)*
- **N20 — Worker never terminated; per-request calls have no timeout; pending map unbounded.** `replica/workerStorePort.ts:112-115` `terminate()` only on the probe-fallback (`storePortResolver.ts:71`), never for the adopted worker; `call()` (`:143-155`) settles only on matching response or `error` event; `pending` Map (`:61-64`) evicts only on settle/fail. *Failure:* a silently-dropped worker reply (no `error` event) leaves the promise unsettled → the serialized store queue wedges (client hard-freeze); pending map accumulates. *(Principle VI / liveness.)*
- **N21 — Web outbox store uncapped; no `QuotaExceededError` path.** `replica/outboxStore.ts` has no size bound/eviction (removed only on retirement drain, `entityStoreAdapter.ts:792-798`); no `Quota` handling anywhere. Contrast undo history capped at 50 (`undoHistoryStore.ts:126,194`). *Failure:* persistently-unsettled mutations grow across reloads; a quota-exceeding `put` rejects as opaque error, losing the durable intent. *(Principle IV/XIX.)*
- **N22 — SSE reconnect race → duplicate streams; view-id leak on session reset.** `sessionClient.ts:91-103` `ensureStream` guard is racy across the awaited `ensureSession` hop (resets `streamStarting` at `:98` without setting `unsubscribeStream`); `closeView` early-returns if `!activeSession` (`:269`) leaving ids in `openViewIds` (`:210,225`). *Failure:* overlapping reconnect + subscribe opens two runtime streams that double-dispatch frames; a session reset before `closeView` leaks the server-side view. *(Idempotent reconnect / resource release.)*

## Part C — Watchdog / liveness census (mostly absence)

- **Supervisor:** no health/liveness watchdog — a wedged account runtime (row 5 / N17) is never detected or restarted; there is no heartbeat, and the only "stop" is external per-account abort. `handle` in `ManagedRuntime` is never `.await`ed, so an account-runtime **panic is silently swallowed** (`manager.rs:72-98`) — the account just stops syncing with no process-level signal.
- **Server task:** panic *is* surfaced (`.expect` at `serve.rs:235,240` → `main.rs:73` join `.expect`), but there is no restart.
- **Down-channel bridge:** no liveness detection (row 2 / M9).
- **HTTP:** no idle-session reaper (N9).
- **Web:** no worker liveness timeout (N20); no IDB `onblocked` recovery (N19).

## Part D — Top-10 severity ranking

1. **N1 (+N2, N3) — No shutdown sequence / no signal handling.** Whole-process; every in-flight mutation cut, supervisor accounts killed mid-sync, WAL uncheckpointed. Highest blast radius.
2. **N4 — Blocking SQLite on tokio workers, zero `spawn_blocking`.** Write contention / large sync can starve the async runtime; write Mutex held across whole batch.
3. **Row 5 / N17 — Provider sync + oauth refresh awaited inline in the supervisor `select!`, no timeout, no watchdog.** A hung provider wedges an entire account's loop indefinitely and undetectably.
4. **N11 — OAuth: no HTTP timeout on any IdP call.** Hung IdP wedges the callback handler and the supervisor refresh path (compounds row 5).
5. **N10 — No request/handler timeout, no `TimeoutLayer`.** Any wedged runtime call hangs the HTTP handler.
6. **N8 — Runtime event stream silently drops `Lagged`.** Silent, permanent staleness for `/v1/events` consumers with no gap signal.
7. **N15 — LIMIT-less store reads + startup full-table scan under the write lock.** Unbounded memory/time from large/hostile data; startup stall.
8. **N12 — OAuth `flow_store` unbounded growth.** Unauthenticated, secrets-bearing memory DoS.
9. **N19 + N20 — Web IDB multi-tab upgrade deadlock + worker no-timeout wedge.** Client hard-freeze on deploy or a lost worker reply.
10. **N9 + N6 — SSE session/view server leak (no reaper) + unbounded TLS handshake JoinSet.** Slow resource exhaustion under normal churn / connection floods.

*Below the cut (honorable mentions):* Row 4 push backoff (jitterless/infinite-cycle), Row 10 snooze wall-clock, N5 progress-spawn flood, N13 JWKS stampede, N14 orphaned `.eml`, N16 read-conn peak, N21/N22 web outbox growth + reconnect race, the `open` zombie, and silent Mutex-poison recovery.

Key files: `crates/posthaste-runtime/src/{shutdown.rs,assembly.rs,handle.rs}`, `crates/posthaste-authority-server/src/{push.rs,supervisor/manager.rs,supervisor/runtime.rs,supervisor/sync_flow.rs,oauth/service.rs,oauth/flow_store.rs,oauth/jwks.rs}`, `crates/posthaste-http-api-adapter/src/{serve.rs,tls.rs,api/runtime_stream/*,api/sync_events.rs}`, `crates/posthaste-server/src/main.rs`, `crates/posthaste-store/src/{store.rs,db/connection.rs,outbox.rs,snooze.rs,cache/body_objects.rs}`, `apps/web/src/runtime/{sessionClient.ts,httpAdapter.ts,replica/{replicaDatabase.ts,workerStorePort.ts,outboxStore.ts,entityStoreAdapter.ts}}`.
