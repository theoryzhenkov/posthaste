# AUDIT-L2-architecture-health — consolidated improvement backlog

> **Status: EVIDENCE (2026-07-05).** Synthesis of four parallel deep reviews (Fable) —
> backend runtime, client architecture, data & providers, cross-cutting — deduped and
> ranked. **Confidence tags:** `[xconf]` = independently confirmed by 2–3 reviewers;
> `[repro]` = reasoned from code, needs a live repro to confirm (esp. IMAP paths the
> mock gateway can't exercise). Line cites into the main tree.

## The shape of the debt (executive summary)
The **steady-state pipelines are genuinely well-built** and several claimed invariants
hold under scrutiny (M35 unsettled-guard, atomic prune+cursor, per-message replay
ordering, M34 deadlines, the optimistic convergence kernel, the DS1 *core* JMAP floor
guard). The risk is concentrated in three places:
1. **Mail-safety seams** — DS1 fixed ONE prune path; **3+ sibling absence-delete paths have the same/adjacent mail-loss hole**, plus duplicate-send windows and an IMAP-delete-resurrect bug. This is the beta-critical cluster.
2. **Composition & recovery edges** (client) — install race, worker-respawn-into-empty-store, failed-open/failed-hook paths that **degrade silently instead of surfacing**.
3. **The async/blocking-store boundary** (backend) — the wedge class we fixed narrowly is broader; a *synchronous* store call on the async executor can't be rescued by the arm-budget timeout.
Plus **half-finished migrations** (M49 mail-list collapse, D132 draft upsert, the TS near-end fork, M70–M73) whose tails were deferred while docs kept describing the old world, and **guardrail holes** (Stalwart tests not in CI, no proptest on mail-safety, mcp/asyncapi drift, committed-wasm drift, no schema versioning).

---

## CRITICAL — mail loss / duplication / wedge (beta-blocking)

### Mail-safety (data & providers)
- **DP-C1 [xconf][repro] IMAP hard-delete wipes its own provider coordinates → delete never sent, message resurrects.** The S2 canonical write-through deletes `imap_message_location` in the optimistic write (`store/mutations/projections/delete.rs:43-47` via `commands.rs:216`); flush reads locations back, finds none → `Rejected`→Failed; next IMAP delta re-imports the still-live server UID as new. Fires on *every* IMAP hard-destroy. Mock gateway never reads locations, so tests miss it. **Fix:** optimistic Destroy must NOT delete `imap_message_location` (let settle/sync remove them) or snapshot locations into the op payload. **Verify with a live-IMAP repro first.**
- **DP-C4 [xconf] IMAP absence-derived deletes bypass the DS1 floor guard entirely.** CONDSTORE-delta + per-mailbox full-snapshot derive deletions from a single `UID SEARCH UNDELETED` and emit them as *explicit* `deleted_message_ids` (`gateway/identity.rs:42`, `changed_since.rs:124`); the store's explicit-delete loop (`sync_batch.rs:127-161`) has NO floor guard (DS1 only guards `replace_all_messages`). A truncated/empty-but-Ok SEARCH deletes a mailbox's mail + advances MODSEQ so nothing re-delivers. **This is the sibling the DS1 doc's own TODO named.** **Fix:** mark absence-derived deletes (vs provider-asserted VANISHED) and route them through the floor/empty guard.
- **DP-C3 JMAP mailbox full snapshot: unguarded, unpaginated prune-by-absence (DS1 sibling, one object up).** `sync/mailbox.rs:121` one unpaginated `Mailbox/query` + `replace_all_mailboxes=true`; `prune_mailboxes_absent_from_remote_tx` (`sync_batch.rs:284`) has no floor/completeness/protected guard. A capped/empty-but-Ok listing deletes every local mailbox → memberships cascade → messages become invisible + IMAP forced to full re-sync. **Fix:** paginate + `remote_ids_complete` + floor guard on the mailbox prune.
- **DP-C2 JMAP full-snapshot id pagination is not change-consistent → single-message loss + invisible new mail.** State captured *after* pagination (`sync/email.rs:270`); a concurrent expunge shifts ids across a page boundary → skipped id passes the completeness check, gets pruned locally, never re-delivered. **Fix:** capture JMAP `state` BEFORE the first `Email/query`; page by anchor id not position; restrict prune to `receivedAt` predating the last consistent page.
- **DP-C5 [xconf] JMAP send: inner 30s timeout / mid-response transport loss classified Transient → blind resend (duplicate send).** The 60s uncertain guard is dead code (jmap-client's own `.timeout(30s)` fires first, `live.rs:41`) → `Network`→`Transient`→resend of an already-executed `EmailSubmission`. **Fix:** for `Send`, any error after request bytes were written = **Uncertain**, classify by PHASE not error type (D81 `CallClass::Send` already specifies this).
- **DP-C6 SMTP send: lettre 30s per-command / post-DATA drop → Network → Transient → blind resend (duplicate).** Same phase-vs-type bug (`gateway/send.rs:19`, `smtp/transport.rs:56`). Stable Message-ID doesn't dedup at recipient MTAs. **Fix (shared with DP-C5):** split by phase; during/after DATA = Uncertain.
- **DP-C7 Split-runtime: sync-origin events never recorded to the link down-channel → new-mail push structurally absent in split mode.** Supervisor `publish_events` (`supervisor/shared.rs:116`) doesn't call `record_base` (only the authority one does, `pubsub.rs:24`); the whole sync pipeline publishes through the supervisor version. Only affects `[link] authority_server_url` (split) mode — **not the bundled beta app** → lower beta priority, but a real gap for the W3 path. **Fix:** one publish seam (supervisor delegates to the authority publisher).

### Backend wedge
- **BE-C1 [xconf] Blocking SQLite on the async executor defeats the arm-budget timeout; `update_runtime_overview` blocks while holding 3 status locks → one account stalls ALL accounts.** Multiple sites (`supervisor/shared.rs:545`, `cache.rs:37/70`, every `outbox.rs` state write on the flush path). A synchronous store call never yields, so `tokio::time::timeout` can't fire — the docs claim the arm budget backstops this; it structurally can't (`types.rs:124`). This is the wedge class we fixed narrowly (cache_maintenance), generalized. **Fix:** route every supervisor/flush store touch through the `offload`/`spawn_blocking` seam the sync path uses (lint-forbid sync port calls from async fns); decouple status persistence from the status locks.

### Client composition/recovery
- **CL-C1 [xconf] Worker watchdog respawn resurrects an EMPTY store — state loss disguised as recovery.** `workerStorePort.ts:200` respawns + replays the one timed-out call; the fresh worker has no views/bases/optimism, so replay "succeeds" on emptiness → rows + unsettled folds silently vanish. **Fix:** respawn triggers a full re-seed (re-open all views against the fresh worker) or fail the port → rebuild the controller.
- **CL-C2 [xconf] Install race (R1): a subscription winning the race binds the base HTTP adapter → whole session bypasses the entity store (no ingest/counts/optimism) until reload.** `adapter.ts:193` fire-and-forget install; `linkClient.ts:103` one-shot bind; 5s worker probe widens the window. Documented-but-unfixed (`AUDIT-L2-client-liveness.md:439`); deleting the REST fallback *raised* the stakes. **Fix:** gate first subscribe/`ensureLink` on the install promise, or re-bind on adapter swap.
- **CL-C3 Steady-state counts have no level-trigger + the optimistic/echo path emits no countDelta.** (A1 mitigated mark-read via the enriched echo; recovery via C1 reconcile.) Residual: any missed event freezes a source count until reload, and a fresh `mailboxes` refetch is still shadowed by a stale live entry outside the recovery edge. **Fix:** reseed the live slice from ANY fresh `mailboxes` query (level-triggered), not just `onLinkReestablished`.

---

## HIGH — correctness / duplication / silent-strand (narrower windows)

**Data & providers:** DP-H1 IMAP streamed cross-mailbox delete ignores resuming mailboxes' locations (durable loss); DP-H2 B4 resume never reconciles the committed prefix (permanent ghosts); DP-H3 [xconf] IMAP multi-mailbox move COPY-then-remove re-COPYs on retry (server-visible duplicate on Dovecot/Cyrus/iCloud/Outlook); DP-H4 [xconf] reconcile/protected set snapshotted at sync start → a local create during a long sync is prunable; DP-H5 permanent flush failure with no readback never reverts the canonical write (JMAP strand — contradicts "no strand paths"); DP-H6 [xconf] deterministic create-id does NOT dedup on RFC-8620 servers + IMAP APPEND has no token → twin drafts/sends (needs the un-landed M72 adopt-by-header); DP-H7 WS `send_request` has no deadline → half-open socket wedges the outbox indefinitely (the likely M67 residual hang); DP-H8 IMAP keyword STORE never verifies the UID exists → silent lost flag change; DP-H9 split-runtime T1/W3 confirmed (projection-stripped events + non-message topics never cross the link); DP-H10 draft-registry enqueue is a read-modify-write across two connections (lost update → twin/notFound); DP-H11 sync "forget" ignores unsettled draft ops (contradicts the M69 invariant); DP-H12 mailbox counters drift permanently if membership precedes the mailbox row, no recount path; DP-H13 no schema versioning — trigger/index fixes can never land on existing installs, and M73's table rename has no migration machinery.

**Backend:** BE-H2 [xconf] outbox head-of-line — one poisoned "transient" op wedges the account's whole outbox forever (no attempt cap/backoff/quarantine), silently blocking sends; BE-H3 send-bridge joins durable state through volatile memory (restart/bus-lag strands the client's send verdict → "sending…" forever); BE-H4 arm-budget cancellation mid-flush spuriously parks sends DispatchUncertain; BE-H5 global sync-slot queue wait burns the arm budget → cross-account false degradation (>8 accounts); BE-H6 settlement sink drops disconnect-window settlements on reconnect (contradicts its own contract).

**Client:** CL-H1 fire-and-forget `void enqueue` swallows every settle/re-projection failure → silent optimism strand (not covered by the sync try/catch); CL-H2 runtime is a mutable module-global singleton with import-time WASM side effects (36 files bind directly; the CL-C2 race is a symptom); CL-H3 fold vocabulary has no upsert → draft saves structurally lag-bound (D132); CL-H4 **reply-all close-save drops threading headers** (my traditional-draft pivot regression); CL-H5 **close-prompt save bypasses its own `isPreparingMessage` gate** → provisional/absent threading persisted (my pivot); CL-H6 compose autosave still uses the legacy fire-and-forget `deleteDraft` REST bypass + swallows save failures (silent data loss on close); CL-H7 `connect()` poisons `running` on a failed open → retry connects a link with no frame pump; CL-H8 pending-set hook failures collapse to "empty" → durable mutations silently never replayed.

---

## MEDIUM / LOW (summarized)
Backend: coalesce-vs-flush TOCTOU; non-atomic settlement (crash window reverts UI); opportunistic reapers + zombie forwarder tasks; command-channel backpressure blocks HTTP callers; status flaps; `MutationCancelGuard` may record a false Failed verdict. Client: three row-stores / four count-representations / two discard-paths (M49/D132/2e.3 residuals); `useLiveView` dead; viewDelta two owners; frame reordering across the buffer; no wasm panic hook; layering WINDOW band monotonic (saturates over a multi-day session); command-search smeared across 3 hooks; ~60-prop drill; W1 keep-alive shim untested at the browser boundary. Data: floor-guard refusal still commits the cursor; UIDVALIDITY verified only at plan time; B4 checkpoint advances by requested-not-fetched UIDs; Gmail-over-IMAP destroy isn't permanent; JMAP destroy-redelivery parity gap; body/attachment 60s total deadline on a streaming payload; M27 semaphore no-timeout/barging/reopen-after-close; unbounded `event_log`; FTS fires on every update + unstable rowid; account removal leaves orphans; large-mailbox query perf (concatenated sort key can't use indexes; conversation projection read by nobody).

---

## Consolidation targets (god-modules + patch-clusters)
1. **`outbox.rs` (1234 LOC)** — the one D29 missed; fuses queue/drafts/flush/settlement. **Split it *inside* M70/M71** so it rides the draft rewrite. Live risks in the cluster: DP-H10, DP-H11, DP-H6, and M70 is only ~80% (`draft_message_exists` survives, dual-read at enqueue).
2. **Send/idempotency cluster** (DP-C5/C6/H5/H6, M9/M10 + apply-ledger P8/DS7) — all trace to *classify-by-type-not-phase* + the missing reconcile-by-id step. **One pass: a persistent idempotency ledger + a phase-based send-transport-fate classifier** closes 5+ open items instead of five separate patches.
3. **Client-liveness / reconnect fork** — the Rust side got `LinkNearEnd`; the web TS near-end (`linkClient`/`httpAdapter`/`entityStoreAdapter`) is the un-consolidated fork (D41; M41–M45/M49/M50). Highest-churn area; consolidation is designed but unshipped.
4. **Compose** — two draft-model pivots in a week; zero integration coverage; produced CL-H4/H5/H6. One `buildOutgoingMessage()`, one discard path, one harness scenario.
Also large: `links.rs` (1694), `handle.rs` (1519), `entityStoreAdapter.ts` (1170), `entity_store.rs` (1172).

## Guardrail gaps (test/CI infrastructure)
- **Stalwart integration tests do NOT run in CI** (env-gated, no workflow sets it) → the most safety-critical repro only ran on one machine. **Add a nightly Stalwart-service-container job.**
- **No property/fuzz coverage on the mail-safety prune paths** (zero proptest/quickcheck) — DS1 was found by luck. **Land testkit P5 targeting prune/floor-guard invariants** (would have caught DP-C2/C3/C4).
- **openapi→schema.gen.ts drift closed for web, NOT mcp** (mcp regenerates instead of diffing).
- **AsyncAPI gap-frame drift (P11)** — hand-maintained; the contract test checks topics not frame shapes.
- **Committed `.wasm` can go stale** — CI diffs only the JS glue. Commit a source fingerprint.
- **No SQLite schema versioning** (`user_version`/migrations) — blocks any trigger/index fix on existing installs (DP-H12/H13) and M73's rename.

## Doc-vs-code drift (extensive — the docs corpus IS the backlog, so this matters)
BETA-READINESS stale (B4 "not yet worked" but landed; DS3 fixed but marked open; "no strand paths" contradicted; the DS1 "audit IMAP" clause = DP-C4/C3 unaddressed); RFC-L2-drafts still specs the deleted autosave-rotation model; ~350 dangling `@spec` anchors + several nonexistent DESIGN docs cited from code; M48 "landed w/ Playwright smoke" that is plan-only; multiple L2/L3 client-link doc claims false. **A resolving `@spec` lint would catch this class.**

---

## Recommended priority order (for beta)
1. **Mail-safety cluster first** — it's unforgivable in a mail client and it's what the rapid patching missed. Repro-confirm then fix **DP-C1** (IMAP delete resurrect), **DP-C4+C3** (extend the DS1 floor guard to the sibling absence-delete + mailbox-prune paths), **DP-C2** (JMAP snapshot state-before-pagination). Land **proptest (P5)** alongside so they can't regress.
2. **Duplicate-send (DP-C5+C6)** — one phase-based classifier; trust-critical, affects everyone.
3. **The two composition-edge Criticals** — **CL-C1** (worker-respawn re-seed) + **CL-C2** (gate the install race). Silent whole-session deadness.
4. **The async/blocking-store wedge (BE-C1)** — route store calls off the async executor; completes the wedge fix.
5. **My recent regressions** — CL-H4/H5/H6 (reply-all threading, gate bypass, autosave-delete REST bypass) — small, near-term.
6. **Guardrail gaps** — Stalwart-in-CI + proptest are the highest-leverage (they make 1 durable).
7. Then the consolidation passes (outbox split into M70/M71; the send-idempotency ledger; the client-liveness fork).
</content>
