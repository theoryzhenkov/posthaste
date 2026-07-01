---
scope: L2
summary: "Lifecycle-debt register from the 2026-07-01 principle audits (git e5eae6229, var/audit-{runtime,backend,client}.md): missing deadlines, reconcilers, shutdown drain, backoff jitter, and growth bounds. Orthogonal to the architecture-cleanup RFC's crate topology, but several items sit in files the M-steps rewrite — fix opportunistically at the marked steps or as follow-up units."
modified: 2026-07-02
reviewed: 2026-07-02
lifecycle: ephemeral
type: ISSUE
status: open
priority: HIGH
depends:
  - path: eph/RFC-L2-architecture-cleanup
---

# Runtime lifecycle debt (from the 2026-07-01 audits)

The full audits live at git `e5eae6229` (`var/audit-{runtime,backend,client}.md`,
since removed from the tree). This register carries the actionable findings so
they survive the refactor. None of them block the RFC's M-steps; the **M-step
column marks where the refactor already rewrites the file** — fixing there
avoids double churn.

| # | Finding | Principle | Evidence | Opportunistic M-step |
|---|---------|-----------|----------|---------------------|
| 1 | No deadline on the remote authority-server link: `reqwest::Client::new()` without timeouts; every `post_link` + the SSE `subscribe` wait unbounded — a hung authority server wedges the mutation pipeline forever | VI | `posthaste-runtime/src/transport.rs:56,88,196` | M4 renames `RemoteBackend → RemoteAuthorityServer` in this file |
| 2 | No reconnect/reconciler behind the down-channel: `run_backend_down_channel` is fire-and-forget; stream end = permanent silent staleness (outbox never retires, cache never evicts) | VIII, XIV | `posthaste-runtime/src/read.rs:288-310`; spawned unretained `build.rs:281` | M3/D29 splits `build.rs`; the reconciler is the missing level-triggered edge |
| 3 | Shutdown is a stub: `AtomicBool` nobody checks; account tasks hard-`abort()`ed mid-operation; no drain deadline | XII | `posthaste-runtime/src/build.rs:1253-1256`; `supervisor/manager.rs:95-99` | M3/D29 extracts `shutdown.rs` — implement, don't just move |
| 4 | Push reconnect: jitterless exponential backoff, unbounded retry, fallback cycle resets the failure counter (loops forever), no poll-only degraded tier | VII, XIX | `authority-runtime/src/push.rs:104-155` | — (follow-up unit) |
| 5 | Account sync awaited inline in the supervisor `select!` loop with no timeout — a hung provider blocks push/commands/ticks for that account | V, VI | `supervisor/sync_flow.rs:172`; `runtime.rs:67` | — (follow-up unit) |
| 6 | IMAP: zero `tokio::time::timeout` in the whole crate; unbounded `BODY.PEEK[]`/`uid_search` allocations from a hostile server | VI, VII, IV | `posthaste-imap/src/{discovery.rs:123,idle.rs:40,body.rs:65,fetch/headers.rs:18}` | — (follow-up unit) |
| 7 | Domain outbox retries `FlushError::Transient` with no backoff and no max-attempts | XIX, VII | `posthaste-domain/src/service/outbox.rs:460-466` | M1 moves this file into `domain-service` |
| 8 | Web client: flat 1s reconnect forever (no backoff/jitter/breaker, no degraded-mode UI); unbounded request-waiter queue with no timeout | VI, VII, XIX | `apps/web/src/runtime/sessionClient.ts:40-55`; `core.ts:121-145` | — (follow-up unit) |
| 9 | SSE frames and HTTP responses cast unchecked at the network boundary (`JSON.parse(…) as RuntimeFrame`) | III, IV | `httpAdapter.ts:246,336`; `core.ts:218` | M5's typed vocabulary is the natural moment to add a parse boundary client-side |
| 10 | Snooze scheduler compares wall-clock (`SystemTime::now`) for due-times | XXIII | `supervisor/runtime.rs:241-244` | — (follow-up unit; small) |

Related open issues in the main tree's `docs/issues/` that intersect M-steps:
`L2-runtime-nearnode-remote-seam` (items B/C/D — account-scoped overlay,
phantom seq gaps; item D's dead `Conflict` arm is D16, executed at M5),
`L2-engine-absorption-footguns` (retire invariant lives only in
`EntityStore::settle` — D9/M6 lifts it into the engine seam),
`L2-store-correctness-grabbag` (`link-replica` `in_range`/GC),
`L2-legacy-leftover-structures` (item B, ungated invalidation storm).
