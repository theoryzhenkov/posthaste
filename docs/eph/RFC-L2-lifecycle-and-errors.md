---
scope: L2
summary: "RFC — lifecycle & error-taxonomy hardening: the ordered shutdown/teardown sequence, deadline discipline, bounded growth, drain/liveness, and one typed retryability/terminality vocabulary with sanitize-at-boundary error hygiene. Draft for ratification. Evidence base: AUDIT-L2-lifecycle-resources (N1-N22 + top-10) + L2-runtime-lifecycle-debt + the /v1 boundary findings. Decisions D60+; migration M20+. The next refactor wave after the architecture-cleanup M-track."
modified: 2026-07-03
reviewed: 2026-07-03
lifecycle: ephemeral
type: RFC
state: draft
depends:
  - path: eph/AUDIT-L2-lifecycle-resources
  - path: issues/L2-runtime-lifecycle-debt
  - path: eph/RFC-L2-architecture-cleanup
  - path: eph/RFC-L2-scripting
  - path: architecture/L2-crate-topology
dependents: []
---

# RFC — L2 Lifecycle & Errors

Status: **draft — for ratification.** This is the drain/outbox for the next
refactor run: two intertwined domains, **lifecycle** (shutdown, deadlines,
bounded growth, drain/liveness) and **errors** (one typed retryability
vocabulary + boundary hygiene). It cites the audit register rather than
re-deriving; every decision carries an evidence pointer, and every rejected
alternative keeps its reason (so it is not re-litigated). It touches no durable
spec and no code until ratified — same four-role discipline as
[RFC-L2-architecture-cleanup](RFC-L2-architecture-cleanup.md).

**Numbering (reserved ranges).** Decisions here are **D60+** — the cleanup RFC
owns D1–D54 and scripting owns D52/D53; **D55–D59 are left as a gap** so neither
track collides. Rejected rows continue the shared R-series at **R5+** (cleanup
holds R1–R4). Migration steps are **M20+** (cleanup holds M0–M10, its next wave
M9; scripting holds S1–S6). Tenet refs point at
`~/.claude/agent/docs/engineering/L1-principles.md`.

**Where a fix is landed or specced elsewhere, this RFC cross-references, never
re-proposes:** the near-end/far-end resilience engine and its deadline+reconnect
policy are M9 (cleanup D40/D41); the `/v1/events` gap-frame that closes N8 is
specced in [RFC-L2-scripting §3–§4 + S2](RFC-L2-scripting.md) (the fact-carrying
channel / tap); D49's `Degraded` is a cleanup accepted decision. This RFC owns
what those do not: the process teardown sequence, deadline discipline outside
the link engine, growth bounds, and the error taxonomy.

## 1. Evidence summary

All rows below are from [AUDIT-L2-lifecycle-resources](AUDIT-L2-lifecycle-resources.md)
(re-verified register + N1–N22 + watchdog census Part C + severity Part D) and
[L2-runtime-lifecycle-debt](../issues/L2-runtime-lifecycle-debt.md) (rows 1–10).
The `/v1`-boundary error findings were re-confirmed against current code during
this draft (paths cited inline in §3B).

**A. Lifecycle.**

- **Shutdown is absent, not merely weak.** No signal handler anywhere in the
  repo; three `main`s just `.await` the join handle (audit N1); `axum::serve`
  has no `.with_graceful_shutdown` (N1); the supervisor has no `stop_all`/`Drop`
  and only ever hard-`abort()`s per account (N2); the store is never
  closed/checkpointed (N3); the down-channel bridge task is untracked (N7). The
  one `RuntimeShutdownHandle` that exists only flips an `AtomicBool` **and is
  invoked in no production path** — tests only (audit row 3, N1). **Top-10 #1.**
- **Deadlines are the exception, not the rule.** Zero `spawn_blocking` in any
  backend crate; the write `Mutex` is held across a whole batch closure (N4,
  top-10 #2). No `TimeoutLayer`, all HTTP handler awaits unbounded (N10). OAuth
  builds a `reqwest::Client` with no `.timeout()` and re-creates it per request;
  JWKS has a stampede + hard-fail with no stale fallback (N11/N13). The
  supervisor `select!` loop inline-awaits provider sync and oauth refresh with
  no `tokio::time::timeout` — a hung provider wedges the account undetectably
  (audit row 5, N17; issue rows 5; top-10 #3/#4/#5).
- **Growth is unbounded in several places.** OAuth `flow_store` grows under a
  callback-less flood — unauth, secrets-bearing (N12, top-10 #8); LIMIT-less
  outbox/snooze reads + a startup full-table scan under the write lock (N15,
  top-10 #7); read-connection peak uncapped (N16); progress-spawn flood (N5);
  unbounded TLS-handshake `JoinSet` (N6).
- **Drain / liveness gaps.** `/v1/events` silently drops on broadcast `Lagged`
  (N8 — **fix owned by scripting S2**, referenced not duplicated); SSE
  session/view server-side leak with no idle reaper (N9); the watchdog census
  (Part C) is mostly *absence* — an account-runtime **panic is silently
  swallowed** because its `JoinHandle` is never awaited.
- **The web-client trio (N18–N22).** No unload lifecycle / unawaited durability
  write / SSE kept open while hidden (N18); IndexedDB multi-tab upgrade deadlock
  (N19); worker never terminated, per-call no timeout, unbounded pending map
  (N20); uncapped web outbox with no `QuotaExceededError` path (N21); SSE
  reconnect race → duplicate streams + view-id leak (N22).

**B. Errors (the /v1 boundary, re-confirmed this draft).**

- **Zero operator logging on 5xx.** The whole HTTP adapter has exactly one
  `error!` and it is in `tls.rs:132` (tcp-accept); no `on_failure` layer, no
  `error!` at 5xx construction — every 500 is silent server-side.
- **Raw IO text leaks into 500 bodies.** `api/accounts/logos.rs` builds error
  bodies as `internal_error(format!("… {err}"))` at **lines 63, 70, 132, 206**
  (create_dir_all / write / read / remove_file) — the `io::Error` `Display`
  string ships to the client.
- **`internal_error` is a catch-all.** `api/account_support/events.rs:90`
  `internal_error(String)` + `errors.rs` collapsing `TransportDisconnected |
  Internal → InternalError` (and `CannotCalculateChanges | StorageFailure |
  StorageCorrupted | ConfigIo → 500`) flattens distinct failure classes into
  one opaque code.
- **`from_service_error` surfaces `error.to_string()` unfiltered** into the
  response `message` (`errors.rs:107,113`) — no sanitation between the domain
  error's `Display` and the wire body.
- **Authz deny reasons are computed then discarded.** `runtime_stream/mutations.rs:130`
  matches `authz::Decision::Deny(_)` and throws the reason away; the same inline
  `Deny → ApiError` mapping is duplicated across the runtime_stream handlers.
- **Retryability is a string bucket.** `domain-service/service/outbox.rs:30`
  `classify_gateway_error` folds every `GatewayError` into
  `FlushError::Transient(String)` / `Permanent(String)` — the retry/terminality
  decision is carried as a free-text message; `other => Permanent(other.to_string())`
  is the lossy default. The near-end engine's permanent-vs-transient logic and
  D49's `Degraded` classify the same facts independently.

## 2. Principles at stake

- **XII (graceful teardown)** — a process that is `kill -9`'d loses in-flight
  work and leaves the store un-checkpointed; shutdown is a first-class sequence,
  not an afterthought. (N1/N2/N3/N7.)
- **VI (a deadline on every await) / V (the operation owns its deadline)** — an
  unbounded `.await` on IO or a lock is a latent hang. (N4/N10/N11/N13, row 5/N17.)
- **IV (bound what you allocate) / VII (release what you acquire)** — every
  collection that grows from input needs an explicit cap; every acquired handle
  (session, connection, `.eml`, child) needs a release path. (N12/N15/N16/N5/N6/N9/N14.)
- **VIII (no silent resync)** — a dropped event needs a visible gap, never a
  silent hole. (N8 — owned by scripting S2.)
- **X (idempotent recovery)** — restart-recovery is real but is not free and not
  complete; it does not license skipping teardown. (Bears on N3/N14.)
- **XIII (a value that lies is worse than none)** — an error code that collapses
  seven classes, or a 500 body that leaks `io::Error` internals, deceives the
  operator and the caller respectively. (§1B.)
- **XIV (one shared fact, one type)** — retryability/terminality is one fact
  classified in three places today; it is one typed vocabulary. (D70.)
- **XV (name the boundary once) / XVI (narrow interface)** — sanitize at the
  boundary; each component owns one `shutdown(deadline)`. (D60/D72.)
- **XIX (degrade under pressure)** — bounded backoff, stale-cache fallback, a
  poll-only tier — not infinite retry or hard-fail. (N13, row 4; largely M9.)
- **XXII (name reveals intent) / XXIV (encode the invariant as a test)** — a
  deliberate fail-closed panic gets a greppable named macro; the frontier/growth
  bounds get verification greps. (D73, §5.)

## 3. Decisions

Reserved range note: **D55–D59 unused** (gap after cleanup D54 / scripting D52–D53).

### 3A. Lifecycle

| Ref | Decision | Rationale (tenet) | Status |
|-----|----------|-------------------|--------|
| D60 | **The teardown sequence is one named component with an ordered contract.** Introduce `ShutdownSequence` owned by the composition root (`posthaste-server`, and the equivalent seam in `posthaste-runtimed`), driven by a single **signal handler** (`tokio::signal::ctrl_c` + a unix `SIGTERM` stream) that fires a shared `tokio_util::sync::CancellationToken`. The sequence is ordered and each step is deadline-bounded under one top-level budget: **(1)** stop accepting — axum `.with_graceful_shutdown(token)` (audit N1, `serve.rs:229-243`); **(2)** drain in-flight HTTP/SSE to a sub-deadline; **(3)** `AccountSupervisor::stop_all` (D61); **(4)** invoke the runtime shutdown handle **for real** — today `RuntimeShutdownHandle::shutdown` (`shutdown.rs:19-31`) only `stopped.store(true)` and has **no production caller** (audit row 3/N1); wire it to stop the tracked tasks incl. the down-channel bridge (N7, `assembly.rs:276`); **(5)** `DatabaseStore::close` + WAL checkpoint (D62). Each long-lived component exposes exactly one `shutdown(deadline)` / `stop_all(deadline)`; the composition root sequences them (XVI — no component reaches across the boundary). The bundled `posthaste-authority-runtime-server` runs the same sequence over both mounted nodes. | XII; XVI; XV | proposed |
| D61 | **`AccountSupervisor::stop_all(deadline)` replaces per-account `abort()`, with a join and a watchdog.** The supervisor (`supervisor/manager.rs:72-98`, `types.rs:149`) gains cooperative stop: signal each account's `select!` loop to exit (a per-account `CancellationToken` arm), then `join` the `JoinHandle`s under a shared deadline; only past the deadline does it fall back to `abort()`. **Liveness (Part C):** the supervisor **awaits/monitors** each account `JoinHandle` so a panic is surfaced (`error!` + a `rule.fired`-style liveness fact / metric) instead of silently swallowed; a panicked account is restarted under a bounded-backoff policy (shares the backoff vocabulary with row 4 / the M9 engine — see D66/open-Q2). Cures N2 + the swallowed-panic census. | XII; VII; XXIV | proposed |
| D62 | **`DatabaseStore::close()` with an explicit WAL checkpoint is a component method.** Add a `close`/`checkpoint` to `posthaste-store` (`store.rs`) that runs `PRAGMA wal_checkpoint(TRUNCATE)` and drains the read pool; called as teardown step (5). Fixes N3 (WAL grows across signal-kills under `synchronous=NORMAL`, `connection.rs:18,21`). Pairs with N14: staging the content-addressed `.eml` *inside* the write txn's success path (or a compensating sweep) so a rolled-back txn leaves no orphan (`store.rs:230-231`, `mutations/commands.rs:114-116`). | XII; VII | proposed |
| D63 | **SQLite runs off the async workers, and the write `Mutex` is scoped to the write, not the batch.** All blocking `rusqlite` work moves behind `tokio::task::spawn_blocking` (or a dedicated blocking pool sized to the connection budget); the write `Mutex` (`store.rs:196-208`) is held only around the txn, not across an entire `apply_sync_batch` closure — two independent defects in N4 (top-10 #2). `busy_timeout(5s)` stays as the floor, not the strategy. | V; VI | proposed |
| D64 | **A `TimeoutLayer` at the HTTP boundary + explicit deadlines on the streaming handlers.** Add `tower_http::timeout::TimeoutLayer` to the `/v1` stack (`serve.rs:194`, today Trace+Cors only) for unary handlers; SSE/stream handlers (`runtime_stream/sessions.rs:85-93`, `sync_events.rs:120-124`, `mutations.rs:46-51`, `views.rs`) that a blanket timeout would wrongly cut instead take an **explicit per-await deadline** on the wedge-prone runtime call. Cures N10 (top-10 #5). | VI | proposed |
| D65 | **OAuth uses one shared timed `reqwest::Client` and single-flights JWKS with a stale-cache fallback.** Build the client **once** with `.timeout()` + `.connect_timeout()` (replacing the per-request rebuilds at `oauth_routes/handlers.rs:38,113,195`, `supervisor/connection.rs:284`; base at `oauth/service.rs:20-26`); every IdP call (token exchange, refresh, OIDC discovery, JWKS — `service.rs:75-139`, `jwks.rs:33-54`) inherits the deadline. JWKS validation single-flights the fetch (a `tokio::sync` guard around `jwks.rs:12-20`) and falls back to the last-good cache on fetch error rather than failing the whole code exchange. Cures N11 (top-10 #4) + N13. | VI; VII; XIX | proposed |
| D66 | **The supervisor `select!` loop bounds every inline await.** Every provider/oauth await inside `supervisor/runtime.rs:67-121` and `:398-445` (`process_sync_trigger_with_state`, backfill, `handle_oauth_refresh_tick`'s `resolve_secret`/`ensure_connection`) is wrapped in `tokio::time::timeout`; a timeout degrades that account (not the loop) and feeds the watchdog (D61). This is the deadline layer *inside* the supervisor; the link-transport resilience policy (deadlines + jittered reconnect + reconciler) is the M9 `LinkNearEnd` engine (cleanup D40/D41) — **referenced, not re-specced here.** Cures audit row 5 / N17 + issue row 5 (top-10 #3). Also fixes row 10 / N10-adjacent snooze wall-clock (`runtime.rs:241-244`) to a monotonic due-comparison (XXIII) as a rider. | V; VI; XIX | proposed |
| D67 | **Every unbounded collection gets a typed bound.** One decision, enumerated fixes, each independently landable: **(a)** `oauth/flow_store.rs` — a size cap + a background TTL sweep (today prune runs only from `begin_completion`, `flow_store.rs:71,97-106`; `insert` from unauth `/oauth/start` never prunes) — closes the unauth secrets-bearing DoS N12 (top-10 #8); **(b)** `outbox.rs:172-212` + `snooze.rs:126-141` gain `LIMIT` + cursor pagination, and the startup repair scans (`cache/body_objects.rs:135-169` under the write lock at `schema.rs:40`) become bounded/batched — N15 (top-10 #7); **(c)** the read-connection peak is capped, not just the retained-idle pool (`store.rs:8,172-190`) — N16; **(d)** the progress-reporter `tokio::spawn` (`sync_flow.rs:19-27`) coalesces to a single in-flight update (also fixing the last-write-wins race) — N5; **(e)** the TLS-handshake `JoinSet` (`tls.rs:87,111`) gets a concurrency cap — N6. | IV; VII; XIX | proposed |
| D68 | **An idle-session reaper releases leaked SSE sessions/views.** A reaper task evicts runtime sessions + open views that have had no live subscriber past a TTL (`runtime_stream/sessions.rs:47`, `views.rs:61` — today freed only on explicit `DELETE`), reusing the M9a TTL/tick machinery (cleanup D48) rather than a new timer. Cures N9 (top-10 #10). **N8 (the `/v1/events` silent `Lagged` drop) is NOT fixed here — its gap-frame is owned by [RFC-L2-scripting §3–§4 + S2](RFC-L2-scripting.md);** this RFC only records the cross-reference so the two tracks don't double-implement. | VII; VIII | proposed |
| D69 | **The web client gets a page-lifecycle, durability, and worker-liveness pass.** Net-new TS work (the parts **not** absorbed by the M9 wasm near-end): **(a)** an unload lifecycle — `visibilitychange`/`pagehide`/`beforeunload` that awaits the outbox durability write and proactively closes the SSE/server session (N18, `outboxStore.ts:72-78`, `httpAdapter.ts:263,325`); **(b)** IndexedDB multi-tab safety — `onblocked` + `connection.onversionchange` on open (N19, `replicaDatabase.ts:32-57`); **(c)** worker liveness — a per-`call` timeout, `terminate()` on adoption teardown, bounded `pending` map (N20, `workerStorePort.ts:112-155`); **(d)** web outbox/pending-set cap + a `QuotaExceededError` path (N21, `outboxStore.ts` → the D54 `pendingSetStore.ts`). **N22 (SSE reconnect race → duplicate streams / view-id leak) is absorbed by cleanup D41** (the near-end moves behind the wasm boundary, retiring the TS reconnect fork) — referenced; see open-Q5 on sequencing this vs D41. | XII; IV; VII | proposed |

### 3B. Errors

| Ref | Decision | Rationale (tenet) | Status |
|-----|----------|-------------------|--------|
| D70 | **Retryability/terminality is one typed vocabulary, not a string bucket.** Introduce a typed classification in `domain-model` (wasm-safe; see open-Q3) — e.g. `enum Terminality { Transient, Permanent }` carried alongside a typed `reason` code, replacing the free-text `FlushError::Transient(String)`/`Permanent(String)` produced by `classify_gateway_error` (`outbox.rs:30`). **One vocabulary, three consumers:** (1) the outbox flush classification, (2) the near-end engine's permanent-vs-transient retry logic (M9 — it consumes the type, does not re-derive it), (3) D49's `Degraded` availability state composes with it. `classify_gateway_error` becomes an exhaustive `match GatewayError → Terminality` with no `other => Permanent(to_string())` catch-all — a new `GatewayError` variant fails to compile until classified (XIV/XXIV). | XIV; III; XIX | proposed |
| D71 | **Conversion edges lose no failure class; each lossy edge is named and fixed.** No conversion collapses distinct failures into an opaque code or a stringified `Display`. Named edges: **(a)** `errors.rs:107` `from_service_error` sets `message: error.to_string()` — replace with a sanitized, code-carrying body (operator detail goes to the log, not the wire, per D72); **(b)** the `internal_error` catch-all (`account_support/events.rs:90`) + the `TransportDisconnected | Internal → InternalError` and the four-way `→ 500` collapses in `errors.rs:200-230` get distinct typed codes so the operator can tell a storage-corruption from a transport-drop; **(c)** `classify_gateway_error`'s string default (D70). The rule: a conversion may *widen* (add context) but never *narrow to opaque* (XIII). | XIII; III; XIV | proposed |
| D72 | **Sanitize at the boundary; log at the boundary; keep deny reasons.** One boundary policy for `posthaste-http-api-adapter`: **(a)** no raw `io::Error`/internal `Display` text in any 5xx body — `logos.rs:63,70,132,206` stop interpolating `{err}` into the response `message` (a generic client-facing message; the detail is logged); **(b)** operator logging via an `on_failure` hook (or `error!` at construction) for **every** 5xx — today the adapter has one `error!` total (`tls.rs:132`), so every 500 is currently silent; **(c)** authz **deny reasons are logged**, not discarded — `runtime_stream/mutations.rs:130` matches `Decision::Deny(reason)` and logs it; **(d)** the duplicated inline `Deny → ApiError` mapping across the runtime_stream handlers is factored into one helper. | XV; XIII; XIV | proposed |
| D73 | **Deliberate fail-closed panics get a named, greppable, documented macro.** A `fail_closed!(reason)` macro (or a `FailClosed` newtype over `panic!`) marks every *intentional* fail-closed abort — the auth-construction panic block extracted in cleanup D27 (`LinkAuth::from_daemon_settings`) is the exemplar. The macro logs the reason at `error!` before aborting and is `grep`-discoverable (`grep -rn 'fail_closed!'`), so the fail-closed surface is enumerable and audit-able, distinct from an incidental `unwrap`/`expect`. Panic policy is documented once beside the macro. | XXII; XXIV; XIII | proposed |

## 4. Rejected alternatives

| Ref | Rejected | Reason (tenet) |
|-----|----------|----------------|
| R5 | **Rely on next-startup recovery as the shutdown story** ("SIGTERM kills it; recovery cleans up"). | Recovery is real but neither free nor complete: N3 leaves the WAL uncheckpointed and growing, N14 leaves orphaned `.eml`, in-flight mutations/SSE are cut mid-write, and a partially-applied sync is deferred to the next boot. XII: teardown is owned, not delegated to the crash path. X: idempotent recovery does not license skipping the ordered stop. |
| R6 | **Keep per-account `abort()` as the stop mechanism, just add a `join` after it.** | `abort()` cancels at an arbitrary `.await` point — mid sync-batch or mid outbox-flush. Joining an aborted task does not make the partial write clean. D61 requires *cooperative* cancellation (a token arm in the `select!`) with `abort()` only as the post-deadline fallback. (XII.) |
| R7 | **Fix N4 with `spawn_blocking` alone, leaving the write `Mutex` held across the batch.** | Two independent defects: blocking-on-workers *and* an over-wide critical section. `spawn_blocking` stops starving the tokio runtime but the batch-wide `Mutex` still serializes every writer for the batch duration. D63 must do both. (V/VI.) |
| R8 | **Keep string-prefix / substring retryability buckets** (the `FlushError::Transient(String)` status quo, and the near-end engine matching on message text). | Retryability is a decision, not a message; deriving it by string inspection drifts silently as providers change wording and duplicates the logic per site. D70 makes it typed data classified once. (XIV/XIII.) |
| R9 | **Keep a single `internal_error`/`InternalError` catch-all for all 5xx and just bolt logging on.** | Logging the collapse does not un-collapse it for the caller or the metric: storage-corruption, transport-drop, and a config-IO failure are operationally different and want different codes/alerts. D71 requires distinct typed codes. (XIII.) |
| R10 | **A separate supervisor health-check thread that pings each account.** | The `JoinHandle` already *is* the liveness signal — awaiting/monitoring it (D61) surfaces panic and exit for free; a parallel pinger duplicates state and can itself wedge. Monitor what you already own. (XIV/XX.) |
| R11 | **Sanitize errors globally in one `IntoResponse` filter instead of at construction.** | A blanket scrubber cannot tell an operator-only detail from a caller-relevant one and tends to over- or under-redact; worse, it discards the log-worthy detail before it is logged. D72 sanitizes *and logs* at the point the 5xx is constructed, where the class is known. (XV/XIII.) |

## 5. Migration steps

M-numbered from M20. Bottom-up along the dep graph; each is one landable unit
leaving the workspace green (`cargo check --workspace` + `cargo test`; wasm
frontier check on any frontier crate touched; web build on any web unit). Each
row lists its **gate** and a **verification grep** (the greppable proof the
defect is gone). Sequencing invariants at the foot.

| Step | What lands | Decisions | Gate + verification grep |
|------|-----------|-----------|--------------------------|
| M20 | **Shutdown sequence + signal handler + graceful axum**: `ShutdownSequence` in the composition root; `ctrl_c`+`SIGTERM` → `CancellationToken`; `.with_graceful_shutdown`; wire the dead `RuntimeShutdownHandle` incl. the down-channel bridge. | D60 (N1/N7) | Gate: SIGTERM integration test (in-flight request completes, store closes). Grep: `rg 'with_graceful_shutdown'` ≥1; `rg 'ctrl_c\|signal::unix'` ≥1; `RuntimeShutdownHandle::shutdown` has a non-test caller. |
| M21 | **Supervisor `stop_all` + cooperative cancel + panic-surfacing watchdog** replacing lone `abort()`. | D61 (N2 + Part C) | Gate: stop-all test joins under deadline; panic-in-account test surfaces an error, not silence. Grep: `rg 'fn stop_all'` ≥1; no `.abort()` as the *sole* stop path; account `JoinHandle` is awaited/monitored. |
| M22 | **`DatabaseStore::close` + WAL checkpoint**; N14 orphan-`.eml` compensation. | D62 (N3/N14) | Gate: post-close WAL is truncated; rolled-back txn leaves no orphan body. Grep: `rg 'wal_checkpoint'` ≥1; `close` called in the teardown sequence. |
| M23 | **SQLite off async workers**: `spawn_blocking` for rusqlite; write `Mutex` scoped to the txn, not the batch. | D63 (N4) | Gate: workspace tests green; a large write no longer blocks a concurrent read on the runtime. Grep: `rg 'spawn_blocking' crates/posthaste-store` ≥1; the write-lock guard does not span `apply_sync_batch`. |
| M24 | **HTTP `TimeoutLayer` + explicit stream-handler deadlines.** | D64 (N10) | Gate: a wedged runtime call returns a timeout, not a hang. Grep: `rg 'TimeoutLayer'` ≥1; each SSE handler's runtime await is deadline-wrapped. |
| M25 | **OAuth: one shared timed client + single-flight JWKS + stale fallback.** | D65 (N11/N13) | Gate: hung-IdP test returns bounded; concurrent cold-cache validations issue one fetch. Grep: `rg '\.timeout\(' crates/posthaste-authority-server/src/oauth` ≥1; no per-request `ClientBuilder` in `handlers.rs`. |
| M26 | **Supervisor select-loop timeouts** on every provider/oauth await; monotonic snooze (row 10 rider). | D66 (row 5/N17) | Gate: hung-provider test degrades one account, loop stays responsive. Grep: `rg 'time::timeout' crates/posthaste-authority-server/src/supervisor` ≥1; `rg 'UNIX_EPOCH' supervisor/runtime.rs` → 0 for the due-comparison. |
| M27 | **Bounded growth batch** (independently landable sub-units a–e). | D67 (N12/N15/N16/N5/N6) | Gate: flood tests bound memory; large-outbox read is paginated. Grep: `rg 'LIMIT' crates/posthaste-store/src/{outbox,snooze}.rs` ≥1; a `flow_store` sweep task exists; the TLS `JoinSet` has a cap constant. |
| M28 | **Idle-session reaper** for SSE sessions/views (reuses D48 TTL machinery). | D68 (N9) | Gate: open→stream→disconnect-without-DELETE is reaped after TTL. Grep: a reaper task over the session registry; `rg 'reap\|idle.*ttl'` in `runtime_stream`. |
| M29 | **Typed retryability vocabulary + conversion-edge hygiene**: the `Terminality`/reason type; `classify_gateway_error` exhaustive; `from_service_error` + `internal_error` collapses replaced with distinct codes. | D70, D71 | Gate: adding a `GatewayError` variant fails to compile until classified. Grep: no `FlushError::Transient(String)` free-text bucket / no `other => .*to_string()` in `classify_gateway_error`; `rg 'enum Terminality'` ≥1. |
| M30 | **Boundary sanitation + operator logging + deny reasons + `fail_closed!`**: no `{err}` in 5xx bodies; `error!`/`on_failure` on every 5xx; log `Decision::Deny(reason)`; factor the duplicated auth mapping; the panic macro. | D72, D73 | Gate: a 500 test body carries no `io::Error` text and emits a server log. Grep: `rg '\{err\}' logos.rs` → 0 in response bodies; `rg 'fail_closed!'` ≥1; `Decision::Deny\(reason' `≥1 (not `Deny(_)`). |
| M31 | **Web lifecycle/durability/worker/outbox** (N18/N19/N20/N21; N22 deferred to D41). | D69 | Gate: web build + tab-lifecycle tests. Grep: `rg 'visibilitychange\|pagehide' apps/web` ≥1; `rg 'onblocked\|onversionchange' replicaDatabase.ts` ≥1; a per-`call` timeout in `workerStorePort.ts`. |

**Sequencing invariants.** M20 → M21 → M22 are the shutdown chain and land in
order (the sequence references each step). M23–M28 are independent deadline/growth
units, orderable freely after M20. M29 precedes M30 (the typed vocabulary is what
the sanitized codes map onto) but both are independent of the lifecycle chain.
M31 is independent; **re-evaluate its scope after cleanup M9 lands D41** (the
wasm near-end) so the N22-adjacent TS reconnect work is not built twice — see
open-Q5. Rejected ordering: doing the error taxonomy (M29) *after* M30 would
force the boundary codes to be reworked once the vocabulary lands.

## 6. Open questions for the owner

1. **Shutdown deadline budget.** What is the top-level teardown deadline and how
   is it split (HTTP-drain vs supervisor-join vs store-checkpoint)? Under
   systemd/launchd the SIGTERM→SIGKILL window is typically ~10 s — do we target
   that, and is a store-checkpoint allowed to overrun the drain sub-deadline?
2. **Watchdog policy for a panicked account (D61).** Restart-with-bounded-backoff,
   or surface-and-halt that account until operator action? This shares the
   backoff vocabulary with row 4 (push) and the M9 `LinkNearEnd` engine — confirm
   they are one policy, not two.
3. **Home crate for the retryability type (D70).** `domain-model` (wasm-safe,
   shared by `classify_gateway_error` *and* the wasm-pure `link-near-end` engine)
   vs `contract-core`. The near-end engine is on the wasm frontier — the type
   must be serde-only wherever it lands. Preference?
4. **D49 `Degraded` shape.** Is `Degraded` a third terminality class, or an
   orthogonal availability state that *composes* with `Transient`/`Permanent`?
   This RFC assumes the latter (D70); confirm before M29.
5. **Sequencing the web trio (D69) vs the wasm near-end (D41/M9).** Land N18/N19/N21
   as TS fixes now, accepting that some (N22-adjacent reconnect) becomes moot when
   the near-end moves behind wasm — or defer the whole web unit until after M9?
   i.e. how much of M31 is throwaway?
6. **Reaper TTL reuse (D68).** Confirm the idle-session reaper should share the
   D48 acked-cursor/TTL tick machinery rather than introduce its own timer, and
   agree the session-idle TTL value.
