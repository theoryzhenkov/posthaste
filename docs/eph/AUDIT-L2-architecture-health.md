# AUDIT-L2-architecture-health — consolidated improvement backlog

> **Status: RECONCILED (2026-07-10).** Original synthesis was EVIDENCE (2026-07-05) from
> four parallel deep reviews (Fable) — backend runtime, client architecture, data &
> providers, cross-cutting. On 2026-07-10 every CRITICAL and most HIGH items were
> re-verified against HEAD (line cites had drifted; located by content). **Nearly the
> entire CRITICAL cluster is now FIXED** — the beta-critical mail-safety, duplicate-send,
> composition-edge, backend-wedge and lifecycle work all landed. Each item below now
> carries an inline verdict tag: **[✅ FIXED @HEAD]** / **[🔴 OPEN @HEAD]** /
> **[🟠 PARTIAL @HEAD]** with the current anchor. The genuinely-open backlog is small —
> see "Recommended priority order" at the bottom, rewritten to reflect what remains.
>
> **Confidence tags (original):** `[xconf]` = independently confirmed by 2–3 reviewers;
> `[repro]` = reasoned from code, needs a live repro. Line cites into the main tree
> (pre-2026-07-10 anchors are stale; verdict tags carry fresh ones).

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
- **[✅ FIXED @HEAD — `crates/posthaste-store/src/projections/delete.rs:1-71`]** `delete_message_tx` now deliberately preserves `imap_message_location` (torn down later by the sync-owned delete path); flush reads the coordinates back. **DP-C1 [xconf][repro] IMAP hard-delete wipes its own provider coordinates → delete never sent, message resurrects.** The S2 canonical write-through deletes `imap_message_location` in the optimistic write (`store/mutations/projections/delete.rs:43-47` via `commands.rs:216`); flush reads locations back, finds none → `Rejected`→Failed; next IMAP delta re-imports the still-live server UID as new. Fires on *every* IMAP hard-destroy. Mock gateway never reads locations, so tests miss it. **Fix:** optimistic Destroy must NOT delete `imap_message_location` (let settle/sync remove them) or snapshot locations into the op payload. **Verify with a live-IMAP repro first.**
- **[✅ FIXED @HEAD — `crates/posthaste-imap/src/sync.rs:176-254`, `crates/posthaste-store/src/mutations/sync_batch.rs:127-160`]** CONDSTORE delta now partitions deletions into `authoritative_keys` (VANISHED / provider-absent, bypass guard) vs `absence_keys` (routed through a >50% floor guard `imap_absence_prune_allowed_tx`). **DP-C4 [xconf] IMAP absence-derived deletes bypass the DS1 floor guard entirely.** CONDSTORE-delta + per-mailbox full-snapshot derive deletions from a single `UID SEARCH UNDELETED` and emit them as *explicit* `deleted_message_ids` (`gateway/identity.rs:42`, `changed_since.rs:124`); the store's explicit-delete loop (`sync_batch.rs:127-161`) has NO floor guard (DS1 only guards `replace_all_messages`). A truncated/empty-but-Ok SEARCH deletes a mailbox's mail + advances MODSEQ so nothing re-delivers. **This is the sibling the DS1 doc's own TODO named.** **Fix:** mark absence-derived deletes (vs provider-asserted VANISHED) and route them through the floor/empty guard.
- **[✅ FIXED @HEAD — `crates/posthaste-engine/src/sync/mailbox.rs:128-178`, `crates/posthaste-store/src/mutations/sync_batch.rs:370-417`]** mailbox snapshot now pages to exhaustion (`fetch_all_remote_mailbox_ids`), sets `replace_all_mailboxes: remote_ids_complete`, and `prune_mailboxes_absent_from_remote_tx` refuses on empty or >50% prune. **DP-C3 JMAP mailbox full snapshot: unguarded, unpaginated prune-by-absence (DS1 sibling, one object up).** `sync/mailbox.rs:121` one unpaginated `Mailbox/query` + `replace_all_mailboxes=true`; `prune_mailboxes_absent_from_remote_tx` (`sync_batch.rs:284`) has no floor/completeness/protected guard. A capped/empty-but-Ok listing deletes every local mailbox → memberships cascade → messages become invisible + IMAP forced to full re-sync. **Fix:** paginate + `remote_ids_complete` + floor guard on the mailbox prune.
- **[✅ FIXED @HEAD — `crates/posthaste-engine/src/sync/email.rs:231-248,320-328`]** JMAP `state` is now captured BEFORE the first `Email/query` (`fetch_email_state` get-state anchor) and the final cursor uses `state_before`. **DP-C2 JMAP full-snapshot id pagination is not change-consistent → single-message loss + invisible new mail.** State captured *after* pagination (`sync/email.rs:270`); a concurrent expunge shifts ids across a page boundary → skipped id passes the completeness check, gets pruned locally, never re-delivered. **Fix:** capture JMAP `state` BEFORE the first `Email/query`; page by anchor id not position; restrict prune to `receivedAt` predating the last consistent page.
- **[✅ FIXED @HEAD — `crates/posthaste-engine/src/live.rs:167-191,336-342`]** `send_request_dispatch` classifies by phase via `classify_send_dispatch_error`: post-write transport failures → `DispatchUncertain`; only pre-write `.is_connect()` stays retryable. **DP-C5 [xconf] JMAP send: inner 30s timeout / mid-response transport loss classified Transient → blind resend (duplicate send).** The 60s uncertain guard is dead code (jmap-client's own `.timeout(30s)` fires first, `live.rs:41`) → `Network`→`Transient`→resend of an already-executed `EmailSubmission`. **Fix:** for `Send`, any error after request bytes were written = **Uncertain**, classify by PHASE not error type (D81 `CallClass::Send` already specifies this).
- **[✅ FIXED @HEAD — `crates/posthaste-imap/src/gateway/send.rs:6-67`, `crates/posthaste-imap/src/smtp/transport.rs:69-107`]** SMTP send wrapped in `timeout(SEND_TOTAL)`→`DispatchUncertain`; lettre classifier maps mid-exchange i/o + read timeouts to `SmtpDispatchUncertain`, reserving retry for connection setup / permanent codes. **DP-C6 SMTP send: lettre 30s per-command / post-DATA drop → Network → Transient → blind resend (duplicate).** Same phase-vs-type bug (`gateway/send.rs:19`, `smtp/transport.rs:56`). Stable Message-ID doesn't dedup at recipient MTAs. **Fix (shared with DP-C5):** split by phase; during/after DATA = Uncertain.
- **[🔴 OPEN @HEAD — `crates/posthaste-authority-server/src/supervisor/shared.rs:116-120` vs `.../authority_server/pubsub.rs:24-30`]** supervisor `publish_events` still only broadcasts; only the authority publisher calls `record_base`. Split-mode only → low beta priority. **DP-C7 Split-runtime: sync-origin events never recorded to the link down-channel → new-mail push structurally absent in split mode.** Supervisor `publish_events` (`supervisor/shared.rs:116`) doesn't call `record_base` (only the authority one does, `pubsub.rs:24`); the whole sync pipeline publishes through the supervisor version. Only affects `[link] authority_server_url` (split) mode — **not the bundled beta app** → lower beta priority, but a real gap for the W3 path. **Fix:** one publish seam (supervisor delegates to the authority publisher).

### Backend wedge
- **[✅ FIXED @HEAD — `crates/posthaste-domain-service/src/service.rs:113-125` (`offload`/`spawn_blocking`)]** SQLite write stores are offloaded (D63/M23b); status persistence is now an in-memory `append_event`, decoupled from the heavy sync snapshot. **BE-C1 [xconf] Blocking SQLite on the async executor defeats the arm-budget timeout; `update_runtime_overview` blocks while holding 3 status locks → one account stalls ALL accounts.** Multiple sites (`supervisor/shared.rs:545`, `cache.rs:37/70`, every `outbox.rs` state write on the flush path). A synchronous store call never yields, so `tokio::time::timeout` can't fire — the docs claim the arm budget backstops this; it structurally can't (`types.rs:124`). This is the wedge class we fixed narrowly (cache_maintenance), generalized. **Fix:** route every supervisor/flush store touch through the `offload`/`spawn_blocking` seam the sync path uses (lint-forbid sync port calls from async fns); decouple status persistence from the status locks.

### Client composition/recovery
- **[✅ FIXED @HEAD — `apps/web/src/runtime/replica/workerStorePort.ts:166-276`]** respawn now calls `reseedAndReplay` — the re-seed hook rebuilds all views + pending sets from controller state BEFORE replaying the timed-out call. **CL-C1 [xconf] Worker watchdog respawn resurrects an EMPTY store — state loss disguised as recovery.** `workerStorePort.ts:200` respawns + replays the one timed-out call; the fresh worker has no views/bases/optimism, so replay "succeeds" on emptiness → rows + unsettled folds silently vanish. **Fix:** respawn triggers a full re-seed (re-open all views against the fresh worker) or fail the port → rebuild the controller.
- **[✅ FIXED @HEAD — `apps/web/src/runtime/adapter.ts:158-285`, `linkClient.ts:87`]** first `openRuntimeLink` now awaits `whenRuntimeAdapterReady()` → `adapterReadyGate` (`boundedReadyGate(bootEntityStoreInstall)`); no subscribe binds before the entity-store install settles. **CL-C2 [xconf] Install race (R1): a subscription winning the race binds the base HTTP adapter → whole session bypasses the entity store (no ingest/counts/optimism) until reload.** `adapter.ts:193` fire-and-forget install; `linkClient.ts:103` one-shot bind; 5s worker probe widens the window. Documented-but-unfixed (`AUDIT-L2-client-liveness.md:439`); deleting the REST fallback *raised* the stakes. **Fix:** gate first subscribe/`ensureLink` on the install promise, or re-bind on adapter swap.
- **[🟠 PARTIAL @HEAD — `apps/web/src/domain-cache/mailboxCounts.ts:88-150`]** counts moved off the live slice onto react-query with debounced invalidation on `message.updated`, but the reseed is still domain-event-triggered, not level-triggered on ANY fresh `mailboxes` refetch. **CL-C3 Steady-state counts have no level-trigger + the optimistic/echo path emits no countDelta.** (A1 mitigated mark-read via the enriched echo; recovery via C1 reconcile.) Residual: any missed event freezes a source count until reload, and a fresh `mailboxes` refetch is still shadowed by a stale live entry outside the recovery edge. **Fix:** reseed the live slice from ANY fresh `mailboxes` query (level-triggered), not just `onLinkReestablished`.

---

## HIGH — correctness / duplication / silent-strand (narrower windows)

**Data & providers:** DP-H1 IMAP streamed cross-mailbox delete ignores resuming mailboxes' locations (durable loss); DP-H2 B4 resume never reconciles the committed prefix (permanent ghosts); DP-H3 [xconf] IMAP multi-mailbox move COPY-then-remove re-COPYs on retry (server-visible duplicate on Dovecot/Cyrus/iCloud/Outlook); DP-H4 [xconf] reconcile/protected set snapshotted at sync start → a local create during a long sync is prunable; DP-H5 permanent flush failure with no readback never reverts the canonical write (JMAP strand — contradicts "no strand paths"); DP-H6 [xconf] deterministic create-id does NOT dedup on RFC-8620 servers + IMAP APPEND has no token → twin drafts/sends (needs the un-landed M72 adopt-by-header); DP-H7 WS `send_request` has no deadline → half-open socket wedges the outbox indefinitely (the likely M67 residual hang); DP-H8 IMAP keyword STORE never verifies the UID exists → silent lost flag change; DP-H9 split-runtime T1/W3 confirmed (projection-stripped events + non-message topics never cross the link); DP-H10 draft-registry enqueue is a read-modify-write across two connections (lost update → twin/notFound); DP-H11 sync "forget" ignores unsettled draft ops (contradicts the M69 invariant); DP-H12 mailbox counters drift permanently if membership precedes the mailbox row, no recount path; DP-H13 no schema versioning — trigger/index fixes can never land on existing installs, and M73's table rename has no migration machinery.

**Backend:** BE-H2 **[🔴 OPEN @HEAD — `crates/posthaste-domain-service/src/service/outbox/flush.rs`]** [xconf] outbox head-of-line — one poisoned "transient" op wedges the account's whole outbox forever (still no attempt cap/backoff/quarantine/dead-letter), silently blocking sends; BE-H3 send-bridge joins durable state through volatile memory (restart/bus-lag strands the client's send verdict → "sending…" forever); BE-H4 arm-budget cancellation mid-flush spuriously parks sends DispatchUncertain; BE-H5 global sync-slot queue wait burns the arm budget → cross-account false degradation (>8 accounts); BE-H6 settlement sink drops disconnect-window settlements on reconnect (contradicts its own contract).

**Client:** CL-H1 **[🔴 OPEN @HEAD — `apps/web/src/runtime/replica/entityStoreAdapter.ts:411-417,580,674,963,981`]** fire-and-forget `void enqueue` still swallows every settle/re-projection failure (the tail chain maps both success and rejection to `undefined`) → silent optimism strand (not covered by the sync try/catch); CL-H2 runtime is a mutable module-global singleton with import-time WASM side effects (36 files bind directly; the CL-C2 race is a symptom); CL-H3 fold vocabulary has no upsert → draft saves structurally lag-bound (D132); CL-H4 **[✅ FIXED @HEAD — `useComposeAutosave.ts:108-113`, `useComposeSubmission.ts:213-217`]** reply-all save now preserves In-Reply-To/References; CL-H5 **[⚪ NEEDS-CHECK @HEAD]** close-prompt save `isPreparingMessage` gate — the submit path gates but the close-guard save path could not be confirmed either way; CL-H6 **[✅ FIXED @HEAD — `useComposeAutosave.ts:138-153`]** autosave discard now routes through `runtimeMutations.messages.deleteDraft()` (unified path, errors logged not swallowed); CL-H7 `connect()` poisons `running` on a failed open → retry connects a link with no frame pump; CL-H8 pending-set hook failures collapse to "empty" → durable mutations silently never replayed.

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
> **These are the current highest-leverage backlog (2026-07-10).** The CRITICAL fixes above
> all landed by hand with little guarding them; guardrails are what keep them from silently
> regressing under continued agentic development.
- **[✅ FIXED @HEAD — `.github/workflows/ci.yml:60-109`]** Stalwart integration now runs in CI: a REQUIRED `send-path-gate` job + a broader `stalwart-integration` job, both set `POSTHASTE_STALWART_INTEGRATION=1` and provision via `tools/dev/stalwart/provision-ci.sh`.
- **[🔴 OPEN @HEAD]** No property/fuzz coverage on the mail-safety prune paths (still zero proptest/quickcheck) — DS1 was found by luck. **Land testkit P5 targeting prune/floor-guard invariants** (would have caught DP-C2/C3/C4). *← guardrails task 1.*
- **[🟠 PARTIAL @HEAD — `.github/workflows/ci.yml:173-178`]** openapi→schema.gen.ts drift closed for web (`api:check`), NOT mcp (mcp regenerates instead of diff-then-fail).
- **[🔴 OPEN @HEAD]** AsyncAPI gap-frame drift (P11) — hand-maintained; the contract test checks topics not frame shapes.
- **[🟠 PARTIAL @HEAD — `.github/workflows/ci.yml:253-271`]** Committed `.wasm` — CI canonicalizes/hashes the JS glue but does NOT byte-diff the wasm binary or a source fingerprint; toolchain drift can slip through.
- **[🔴 OPEN @HEAD]** No SQLite schema versioning (`user_version`/migrations) — blocks any trigger/index fix on existing installs (DP-H12/H13) and M73's rename. *← guardrails task 2.*
- **[🔴 OPEN @HEAD]** `@spec` anchor lint still absent; 473+ dangling anchors in `apps/web/src` alone — the exact doc-drift that made the 2026-07-10 reconciliation necessary. *← guardrails task 3.*

## Doc-vs-code drift (extensive — the docs corpus IS the backlog, so this matters)
BETA-READINESS stale (B4 "not yet worked" but landed; DS3 fixed but marked open; "no strand paths" contradicted; the DS1 "audit IMAP" clause = DP-C4/C3 unaddressed); RFC-L2-drafts still specs the deleted autosave-rotation model; ~350 dangling `@spec` anchors + several nonexistent DESIGN docs cited from code; M48 "landed w/ Playwright smoke" that is plan-only; multiple L2/L3 client-link doc claims false. **A resolving `@spec` lint would catch this class.**

---

## Recommended priority order — REWRITTEN 2026-07-10 (post-reconciliation)
The original order below (struck through) is **done**: the entire mail-safety cluster
(DP-C1/C2/C3/C4, DS1), duplicate-send (DP-C5/C6), both composition Criticals (CL-C1/C2),
the async/blocking wedge (BE-C1), the compose regressions (CL-H4/H6), lifecycle
(N1–N3/N10–N11), onboarding (B1–B4), and Stalwart-in-CI all landed. What remains:

1. **Guardrails first (highest leverage now).** The code is in good shape; the risk is
   *regression* under continued agentic churn.
   1. **Proptest (P5)** on the prune/floor-guard invariants — the exact class behind DP-C2/C3/C4/DS1, still untested.
   2. **SQLite schema versioning** (`user_version` + a migration runner) — unblocks DP-H12/H13 index/trigger fixes and M73's table rename.
   3. **`@spec` anchor lint** — resolve anchors in CI so the doc corpus (which *is* the backlog) stops drifting.
2. **The two genuinely-open robustness holes.** **CL-H1** (fire-and-forget `void enqueue`
   silently swallows base-frame store failures — the purest "agentic smell hurts robustness"
   instance) and **BE-H2** (poisoned outbox op wedges an account's sends forever — needs an
   attempt cap / backoff / quarantine / dead-letter).
3. **Verify the loose ends** — CL-H5 (close-save gate), DS6 (stale Drafts copy on Sent-move
   failure), CL-C3 (level-triggered count reseed), BE-H4/H6 (settlement edges). PARTIAL / NEEDS-CHECK.
4. **Lower priority / split-mode** — DP-C7 (split-runtime push), mcp openapi diff-then-fail,
   wasm source fingerprint, AsyncAPI frame-shape check.
5. **Quality (not robustness) — defer** — consolidation passes: outbox split (M70/M71),
   send-idempotency ledger unification, the client-liveness TS near-end fork.

<details><summary>Original (2026-07-05) order — superseded, kept for history</summary>

1. ~~**Mail-safety cluster first** — DP-C1, DP-C4+C3, DP-C2. Land proptest (P5) alongside.~~
2. ~~**Duplicate-send (DP-C5+C6)** — one phase-based classifier.~~
3. ~~**The two composition-edge Criticals** — CL-C1 + CL-C2.~~
4. ~~**The async/blocking-store wedge (BE-C1)**.~~
5. ~~**Recent regressions** — CL-H4/H5/H6.~~
6. ~~**Guardrail gaps** — Stalwart-in-CI + proptest.~~
7. ~~Consolidation passes (outbox split; send-idempotency ledger; client-liveness fork).~~
</details>
</content>
