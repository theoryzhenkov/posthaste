---
scope: L2
summary: "IMAP gateway + sync scheduling/supervision robustness audit — findings incl. the P1 data-loss window (full-snapshot prunes in-flight local messages, TODO(S3)), no-timeout/no-IDLE-reissue connection layer, no task supervision, and the P5 flake verdict (real idle-boundary coalescing race, over-strict test). Evidence base for the provider-reliability RFC."
modified: 2026-07-02
reviewed: 2026-07-02
lifecycle: ephemeral
type: AUDIT
state: evidence-complete
depends: []
dependents: []
---

Both subagents delivered. I have complete firsthand coverage of the supervisor/scheduling layer plus the P5 verdict, and detailed subagent evidence for the IMAP gateway and planning. Here is the audit.

---

# IMAP Gateway & Sync Scheduling/Supervision — Robustness Audit

Workspace: `/home/usr.prj_posthaste/src/.workspaces/architecture-cleanup`. Every claim carries file:line evidence. Raw findings only.

## Findings table

### Dimension 1 — IMAP connection robustness

| # | Symptom | Evidence | Failure scenario | Sev |
|---|---------|----------|------------------|-----|
| C1 | No connect / command / read / write timeout anywhere | `imap/src/discovery.rs:121-131` (`ImapClient::rustls(...).await?` with no `timeout`); no `tokio::time::timeout` in the crate outside the IDLE delay constant | Black-holed host or a server that completes TLS then never answers a command hangs the sync task forever; because that task is the account runtime's single select loop (below), the whole account wedges | HIGH |
| C2 | IDLE never re-issued (no ~29-min DONE/re-IDLE, no max duration) | `imap/src/idle.rs:38-40` single un-timed `client.idle(tag).await`; only constant is `idle.rs:14` `IMAP_IDLE_RECONNECT_DELAY = 30s` | Server/NAT silently drops the idle socket past its own timeout → `idle().await` blocks, push notifications silently stop until the OS tears the socket down | HIGH |
| C3 | IDLE recovery is reconnect-only with a fixed 30s delay, no backoff/jitter, no in-crate poll fallback | `idle.rs:63-96` (yield Disconnected → `sleep(30s)` → reconnect loop `:31`) | A server persistently rejecting IDLE is hammered every 30s indefinitely; correctness relies entirely on the external 60s poll | MEDIUM |
| C4 | No connection pool; fresh TCP+TLS+auth+`refresh_capabilities` per operation | `discovery.rs:86-92` called from `gateway/sync.rs:15`, `mutation.rs:37/75/97/118/142`, `body.rs:36`, `gateway/draft.rs:69`, `idle.rs:111` | A burst of small mutations each opens a full new authenticated session → connection storm / provider rate-limit trips | MEDIUM |
| C5 | SMTP transport built with no `.timeout()` | `imap/src/smtp/transport.rs:87-92` | A stalled Sent-append/send hangs on lettre's internal default with no app bound | MEDIUM |
| C6 (+) | TLS verification ON; no danger flags; broken sessions discarded (no pool = no stale reuse) | `discovery.rs:124/127` (`rustls(...,None)` = default roots); `builder_dangerous` only under configured `Plain` (`smtp/transport.rs:78`); sessions are function-local (`error.rs:60-63`) | — | POSITIVE |

### Dimension 2 — Sync planning correctness

| # | Symptom | Evidence | Failure scenario | Sev |
|---|---------|----------|------------------|-----|
| P1 | Full-snapshot sync bypasses the unsettled-optimistic-write guard and can prune in-flight local messages | `imap/src/sync.rs:94` sets `replace_all_messages=true`; guard early-returns exactly then: `domain-service/src/service/sync_ops.rs:31-33` (`if unsettled.is_empty() || batch.replace_all_messages { return; }`, `TODO(S3)`); store then runs `prune_messages_absent_from_remote_tx` (`store/src/mutations/sync_batch.rs:44-56`) | On a plain IMAP4rev1 server (no CONDSTORE/QRESYNC → always `FlagDeltaUnavailable` full snapshot, `planning.rs:50-54`), a locally-created message still in flight is pruned by the next sync | HIGH |
| P2 | `CondstoreDelta` is not incremental — executor ignores `since_modseq`/`after_uid` and full-fetches every header each sync | planner emits `CondstoreDelta{since_modseq, after_uid}` (`domain-service/src/imap/planning.rs:42-47`) but executor destructures `{ .. }` → `fetch_mailbox_header_snapshot_with_client` (`imap/src/gateway/execution.rs:197,220-232`); only QRESYNC uses the real `CHANGEDSINCE` path (`execution.rs:172`) | CONDSTORE-only servers (Dovecot/Fastmail w/o QRESYNC) re-fetch all headers every cycle | MEDIUM |
| P3 | UIDVALIDITY change correctly forces full resync (equality check) | `domain-service/src/imap/planning.rs:22-26` → `sync_state.rs:136` `self.uid_validity == uid_validity`; re-enforced at read/mutate (`body.rs:39-45`, `mutation/validation.rs:31-37`) | — | POSITIVE |
| P4 | UID-gap handling is deliberately robust (full `UID SEARCH UNDELETED` + client-side filter, not `n:*`) | `imap/src/fetch/headers.rs:44-72` (rationale at `:46-48`) | — | POSITIVE |
| P5 | Flags fetched atomically with headers — no lost-flag ordering window | `imap/src/fetch/changed_since.rs:59` (`fetch_item_names(true,…)`), same `SyncBatch` | — | POSITIVE |

### Dimension 2/6 — Checkpoint & cancellation semantics

| # | Symptom | Evidence | Failure scenario | Sev |
|---|---------|----------|------------------|-----|
| K1 | IMAP emits the whole account as ONE batch/txn; no durable partial-sync progress | IMAP doesn't override `sync_streamed`; default emits a single chunk (`domain-service/src/ports/gateway.rs:40-50`); accumulator holds all mailboxes, any per-mailbox `?` discards everything (`imap/src/gateway/execution.rs:52-58`); applied in one txn (`store/src/commands.rs:52-54`) | On a flaky link, a large account never makes durable progress — every restart re-fetches from the last committed watermark; no message loss but poor liveness | MEDIUM |
| K2 (+) | Within a batch, messages + watermark + cursor commit atomically (no torn checkpoint) | `ports/sync_store.rs:104`; `store/src/mutations/sync_batch.rs:195,216-217` all inside one `tx` | — | POSITIVE |
| K3 | IMAP sync has zero cancellation handling; abort drops the future mid-command | no `select!`/timeout/token in crate; sequential awaits `execution.rs:51`, `fetch/headers.rs:113` | Cancel mid-`uid_fetch` tears the connection; in-memory accumulator discarded (all-or-nothing, so at least no orphaned rows for IMAP) | MEDIUM |

### Dimension 3 — Supervisor robustness

| # | Symptom | Evidence | Failure scenario | Sev |
|---|---------|----------|------------------|-----|
| S1 | Account runtime task has NO restart/backoff/watchdog — nobody watches the JoinHandle | `supervisor/manager.rs:72-86` spawns + stores `handle`; `types.rs:149`; grep for `is_finished`/join/`catch_unwind` across the crate = **none**; `run_account_runtime` loop never breaks (`runtime.rs:59-122`) | A panic at any `.await` inside the runtime (e.g. a library panic on a pathological server response, or a poisoned lock) silently kills the account; it stays dead — no status update, no restart — until an external `start_account` (config reload / patch) | HIGH |
| S2 | Single per-account select loop → head-of-line blocking; a long sync starves push/IDLE, OAuth refresh, cache, snooze | `runtime.rs:67-121` (one `tokio::select!`; sync awaited to completion inside `handle_runtime_command`/`handle_poll_tick` before any other branch runs) | A slow provider sync (recall: no timeout, C1) blocks IDLE push consumption and proactive OAuth token refresh for that account for the sync's full duration | MEDIUM |
| S3 (+) | Sync failures are surfaced to user-visible account status + SSE events (UI does learn) | `supervisor/sync_flow.rs:207-236` (`record_sync_failure` + `mark_sync_failure` → `shared.rs:144-167` maps to `AuthError`/`Offline`/`Degraded`, publishes `account.status.changed`) | — | POSITIVE |
| S4 (+) | Generational guard + atomic overview RMW prevent stale/late writes reviving dead status | `types.rs:40-54`, `shared.rs:234-248` (generation check) & `shared.rs:224-333` (atomic RMW); regression-tested `supervisor/tests.rs:146-348` | — | POSITIVE |

### Dimension 4 — Scheduling

| # | Symptom | Evidence | Failure scenario | Sev |
|---|---------|----------|------------------|-----|
| Sc1 | Thundering herd: all accounts started in a tight loop, each fires an immediate Startup sync; poll phases near-aligned; NO jitter/splay anywhere | `build.rs:226-228` (`for source … start_account().await`); initial sync `runtime.rs:48-56`; per-runtime interval phase `= Instant::now()+poll_interval` (`sync_flow.rs:3-8`); grep `jitter|splay|rand` in supervisor = none; default `poll_interval=60s` (`config/daemon.rs:97`) | Boot with N accounts → N concurrent connect+sync storms; steady state → recurring aligned herd every 60s. `MissedTickBehavior::Skip` does not decorrelate | MEDIUM |
| Sc2 | Coalescing has a gap at the idle→syncing boundary (the P5 flake — real race) | `types.rs:105-113` coalesces only when `syncing==true`; gap between `manager.rs:201` `permit.send(TriggerOnly)` and `runtime.rs:170`/`types.rs:116` `begin_cycle`; each mutation fires a trigger (`commands.rs:142→116-119`); triggers buffer in `mpsc::channel(32)` (`manager.rs:65`) | Multiple concurrent triggers observing idle each enqueue a distinct cycle → redundant provider syncs (perf, not correctness). See P5 verdict below | LOW (but real) |
| Sc3 | No priority between user-triggered and scheduled syncs; manual `sync_account` never coalesces | manual `Trigger{reply}` always sent (`manager.rs:140-149`), poll via `interval.tick()` (`runtime.rs:68`); both funnel to the same serialized loop | A stream of manual syncs queue serially behind an in-flight cycle (bounded by channel(32)); no starvation, but no prioritization either | LOW |

### Dimension 5 — Resource bounds

| # | Symptom | Evidence | Failure scenario | Sev |
|---|---------|----------|------------------|-----|
| R1 | Whole folder + whole account buffered in memory, never streamed | UID list buffered `fetch/headers.rs:17`; per-mailbox records `fetch/headers.rs:108,124`; account-level `SyncBatchAccumulator` `execution.rs:49` → single `into_sync_batch` `sync.rs:109`; store stages every raw body up front `store/src/mutations/sync_batch.rs:4-23` | Initial sync of a 100k-message Gmail All-Mail → large transient memory spike + one giant write-lock transaction | MED-HIGH |
| R2 | Unbounded expansion of server-declared VANISHED range | `fetch/changed_since.rs:169-176` `known_uids.iter(NonZeroU32::MAX)` | Malicious/buggy server returns `VANISHED 1:4294967295` → ~4B iterations, CPU/mem DoS, no cap | MEDIUM |
| R3 | Whole raw message literal trusted/allocated from server-declared size | `body.rs:126` copies full `BODY.PEEK[]` into `Vec<u8>`, then parses `body.rs:69`; no ceiling | Oversized/lying literal → large allocation | MEDIUM |
| R4 | No global concurrent-sync bound across accounts | N runtimes = N concurrent sync tasks; only per-account mpsc permit; the one global limiter (`CacheResourceGovernor`, `types.rs:36`) throttles *cache fetches only* — the sync path never consults it | 50 accounts syncing at boot → 50 simultaneous provider connections + fetch loads, unthrottled | MEDIUM |
| R5 (+) | Header FETCH is chunked at 128 UIDs | `fetch.rs:29` `UID_FETCH_CHUNK_SIZE=128`; `fetch/headers.rs:109-110` | — | POSITIVE |
| R6 (+) | No panic on server data — every server field uses `ok_or(...)` typed errors | crate sweep: only 2 `expect` hits, both on constants/tests (`mutation/keywords.rs:65`, `gateway/draft.rs:194` under `#[cfg(test)]`); e.g. `fetch/items.rs:97`, `mailbox.rs:151` | — | POSITIVE |

### Dimension 6 — Cancellation / deletion safety

| # | Symptom | Evidence | Failure scenario | Sev |
|---|---------|----------|------------------|-----|
| D1 | Delete ordering races an in-flight sync: config source + secret deleted BEFORE the runtime is stopped | `mutations/accounts.rs:151-160` (`account_repository.delete()` → removes config + secret, THEN `supervisor.remove_account().await` aborts task); `account_repository.rs:84-90` | Between the two lines an in-flight sync keeps running with its secret/config gone: OAuth refresh fails, and it can still commit NEW store rows for the just-deleted account | MEDIUM |
| D2 | Account deletion does not purge mail store rows (messages/mailboxes/events) | `account_repository.rs:84-90` deletes only config source + managed secret; `supervisor/manager.rs:104-118` `remove_account` clears only overview/gateway/generation | Deleted account leaves orphaned message/event rows in SQLite indefinitely; combined with D1, an aborted sync can add more | MEDIUM |
| D3 | `stop_account` uses `handle.abort()` mid-sync | `manager.rs:90-101` | Abort at an await point; IMAP apply is one sync SQLite txn (no torn txn), but for the multi-chunk JMAP path earlier chunks already committed persist while later ones are abandoned | LOW |

## Top-10 ranking (most to least severe)

1. **C1 — no IMAP connect/command timeout** (any unresponsive provider wedges the account runtime indefinitely; compounds S1/S2).
2. **C2 — no IDLE 29-min re-issue / max duration** (silent half-open connection → push silently dies).
3. **P1 — full-snapshot prunes in-flight local messages** (acknowledged unguarded `TODO(S3)`; data-loss window on plain IMAP servers).
4. **S1 — no runtime-task restart/backoff/watchdog** (one panic permanently kills an account, invisibly).
5. **R1 — whole-account sync buffered in memory** (OOM / long write-lock on large mailboxes; no streaming for IMAP).
6. **Sc1 — thundering herd, no jitter** (boot and steady-state connection/sync storms).
7. **R4 — no global concurrent-sync cap** (unbounded fan-out across accounts).
8. **R2/R3 — unbounded VANISHED/literal allocation from server** (server-driven DoS surface).
9. **D1/D2 — delete races in-flight sync + orphaned store rows** (torn deletion, no row GC).
10. **P2/K1 — CONDSTORE degrades to full fetch + no durable partial progress** (bandwidth + liveness on flaky links/large accounts).

## Verdict — subsystem maturity vs. the link stack

This subsystem is **markedly less mature than the link/replication stack**, and the gap is concentrated in *failure-domain engineering*, not in the happy path. The link stack demonstrates a coherent reliability doctrine: the push transport layer has exponential backoff with a fallback ladder and checkpoint-resume (`push.rs:27,105-154,51-52`), and the runtime registry has a TTL sink reaper, a bounded monotonic seq-backlog, and resume-from-`after_seq` replay (`runtime_registry.rs:15-18,183-205`) — i.e. explicit liveness, leak-reaping, and reconnect-resume primitives. The IMAP/supervisor layer has *islands* of that same rigor — the generational status guard and atomic overview RMW (S4) are genuinely well-built and regression-tested, TLS defaults are safe (C6), server-response parsing is panic-free (R6), and header fetches are chunked (R5) — but the connective tissue that the link stack has is absent here: **no timeouts (C1/C2/C5), no task supervision/restart (S1), no jitter (Sc1), no global concurrency or memory bound (R1/R4), and no streaming/partial-progress for IMAP (K1)**. The supervisor's own comments show the team has been fighting *state-machine* races (lost-wakeup, stuck-"syncing") and winning those, while leaving the *lifecycle and resource* dimensions (a task that just dies; a sync that just hangs; a folder that just buffers) unaddressed. Net: the coalescing/status core is production-grade; the connection/scheduling/resource envelope around it is prototype-grade.

## Verdict on the P5 flake — REAL race, low severity, over-tight test

`rapid_mutation_burst_coalesces_provider_sync_triggers` (`tests/authority_server_handle.rs:2393-2500`) asserts `additional_cycles <= 2` after 15 concurrent flag toggles (`:2496-2499`). The flake is a **genuine race in the coalescing design, not merely a timing artifact** — though its consequence is benign (redundant syncs, not incorrect state).

Evidence chain:
- Each mutation independently fires a trigger: `set_keywords → trigger_outbox_flush → trigger_account_sync(Manual)` (`commands.rs:142,116-119`). 15 mutations = 15 concurrent `trigger_account_sync` calls.
- Coalescing collapses a trigger **only while `syncing==true`**: `coalesce_if_syncing` returns `false` when idle (`types.rs:105-113`), and the caller then enqueues a fresh `TriggerOnly` into `mpsc::channel(32)` (`manager.rs:187-201`).
- `syncing` is not set until the runtime task dequeues the first `TriggerOnly` and calls `begin_cycle` (`runtime.rs:170 → types.rs:116-119`). Between `manager.rs:201` `permit.send(...)` and that `begin_cycle`, the flag stays `false`.
- Therefore **every concurrent trigger that observes the idle window enqueues its own distinct cycle**; the mpsc buffer holds them and the runtime runs them serially — one full provider sync each. Coalescing only ever collapses the triggers that arrive *after* the first cycle has begun (into a single `pending`, `types.rs:121-134`).
- Normally the runtime task is scheduled fast enough that only **one** trigger wins the idle race, giving exactly 2 additional cycles (the in-flight one + one coalesced follow-up) — the test passes. Under CI load/scheduler jitter the `begin_cycle` is delayed, **2–3 triggers slip through the idle window as separate channel-buffered cycles**, and `additional_cycles` exceeds 2 → failure.
- The 50ms mock delay (`:2427`) only widens the *in-flight* window (which coalescing already covers); it does nothing for the *idle-entry* window where the flake actually lives. The added regression tests (`types.rs:334-351`; `coalesced_sync_trigger_still_runs_a_follow_up_cycle` `:2624`) cover only the single-trigger lost-wakeup, **not** concurrent multi-trigger admission at idle.

Conclusion: the assertion bound `<= 2` encodes an implicit "at most one idle-race winner" assumption that is **probabilistic, not invariant**, so the test is legitimately flaky against a real (if low-severity) coalescing gap — the coalescer lacks an atomic "claim" at the idle boundary, so N concurrent idle-window triggers can each spawn a redundant cycle. It is neither a pure test artifact nor a correctness bug; it is a real race whose only symptom is wasted provider syncs, exposed by an over-strict deterministic assertion on nondeterministic scheduling.

Key files: `crates/posthaste-authority-server/src/supervisor/{types,manager,runtime,sync_flow,shared,connection,cache}.rs`, `.../mutations/accounts.rs`, `.../authority_server/commands.rs`, `.../build.rs`, `.../push.rs`, `.../runtime_registry.rs`; `crates/posthaste-domain-service/src/imap/planning.rs`, `.../service/sync_ops.rs`, `.../ports/{gateway,progress,sync_store}.rs`; `crates/posthaste-imap/src/{discovery,idle,body}.rs`, `.../gateway/execution.rs`, `.../fetch/{headers,changed_since}.rs`, `.../sync.rs`; `crates/posthaste-store/src/mutations/sync_batch.rs`.
