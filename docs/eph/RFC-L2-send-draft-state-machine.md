# RFC-L2-send-draft-state-machine — collapse the shadow state machines

> **Status (2026-07-10): PROPOSED / DESIGN — not ratified, nothing implemented.**
> Motivated by a **live P0 in the nightly build: clicking Send sends nothing**
> (root cause in §1), and by the broader finding that the draft/send lifecycle is
> patchwork. The core discovery: the two owning models — `OperationState` (a real
> typed state machine with a `can_transition_to` validator) and `DraftRegistry`
> (a real port) — are decent; **every live bug is state that *escaped* its owning
> model into an ad-hoc shadow (a SQL predicate + a frozen clock, four resolution
> seams, a warn-log).** The refactor is to pull each escaped piece back into its
> typed owner.
>
> **Owner decision (2026-07-10):** the clock fix is NOT split into a standalone
> hotfix — it rides Phase 1. Consequence: Phase 1 is on the critical path to
> restore sending, so **M80 (the clock un-fusing) is sequenced to land first and
> fast** within Phase 1.
>
> Extends/supersedes: RFC-L2-drafts (D125–D127 draft lifecycle), RFC-L2-drafts /
> RFC-L2-draft-identity (D135–D141 identity), RFC-L2-provider-reliability
> (D81–D87 send-exactly-once). Pairs with DESIGN-L2-test-taxonomy (the SEND grid)
> and pulls in the parked SQLite schema-versioning guardrail (AUDIT-L2-architecture-health).

## 1. The symptom: nothing sends in nightly (verified)

Every default send is now a *scheduled* send whose release clock can only run slow:

1. **Every send gets a 10s hold.** `DEFAULT_UNDO_SEND_DELAY_SECONDS = 10`
   (`apps/web/src/api/types/settings.ts:41`). `handleSubmit` takes the immediate
   path only if the delay is `<= 0`; otherwise it stamps
   `sendAt = new Date(Date.now() + delay*1000).toISOString()` — the **browser's
   real wall clock** — and routes through the held-outbox path
   (`apps/web/src/components/compose-overlay/useComposeSubmission.ts:260-272`).
2. **The flush gate judges due-ness against a frozen clock.**
   `list_flushable_operations` excludes any op with `send_at > now`
   (`crates/posthaste-store/src/outbox.rs:213`), where `now = outbox_now_rfc3339()`
   — a clock anchored **once** at daemon start and advanced only by `Instant`
   (monotonic) elapsed (`crates/posthaste-domain-service/src/service/outbox/schedule.rs:66-71`).
3. **That clock can only *lag* real time, never lead it** (by design, so a forward
   NTP step can't fire a held send early). But `Instant`/`CLOCK_MONOTONIC` **pauses
   during OS suspend**, so a long-lived daemon that survives laptop sleeps falls
   behind real wall time by the total sleep duration.
4. **Result:** the send is stamped at *real now + 10s* but judged against a clock
   stuck minutes-to-hours behind real now → `send_at <= now` never becomes true.
   The 5s scheduler tick and every flush path use the **same** lagging clock, so
   nothing rescues it. The mail sits in the outbox.

Passes CI, dies in nightly: the scheduled-send tests use `send_at` of year
`2020`/`2999` (multi-year margins,
`crates/posthaste-domain-service/src/service/tests/outbox.rs`), so realistic skew
is never exercised; a fresh daemon has no accumulated lag; only a **long-lived
instance across sleeps** — the nightly build in daily use — hits it.

**Interim mitigation for users:** set the undo-send delay to `0` (Compose
settings) → sends bypass the held path. (Restarting the daemon re-anchors the
clock, until the next sleep.)

## 2. The thesis: state that escaped its owning model

| Escaped state | Where it lives now (the shadow) | Owning model it escaped |
|---|---|---|
| The scheduling / held sub-state | nullable `send_at` column + SQL `WHERE send_at <= now` gate + a frozen clock — invisible to the state enum | `OperationState` (claims `Pending` is flushable; a SQL predicate silently overrides) |
| Draft identity resolution | 4 seams doing their own thing (`unwrap_or_else(key)` fallback, `draft_message_exists` projection probe) *around* the registry | `DraftRegistry` port (`crates/posthaste-domain-service/src/ports/draft_registry.rs`) |
| The "delivered-but-unfiled" outcome | a `moved_to_sent` warn + `Ok(())` (`crates/posthaste-engine/src/live_compose/send.rs:211-217`) | `OperationOutcome` (only `{Applied, Failed}`) |

The P0 is the purest instance: the typed machine says "Pending ⇒ flushable"
(`domain-model/src/model/outbox.rs:105`), but a shadow predicate against a frozen
clock silently overrides it, and the clock that *stamps* the hold (client wall)
differs from the clock that *judges* it (daemon monotonic). The model can't see
its own lie, so the bug is invisible to it.

## 3. Decisions (proposed)

- **D150 — One owning model per lifecycle concern; no shadow state.** Every
  lifecycle fact (is it held? which provider entity? did it file?) must live
  inside a typed owner with an enforced transition/validation surface. No
  lifecycle decision may live in a raw SQL `WHERE` clause, a nullable side
  column read in isolation, or a warn-log.

- **D151 — Scheduling is a first-class state, not a column + gate.** Introduce a
  resting `Scheduled { due }` state:
  `Scheduled(due) ──(due)──► Pending ──► Inflight ──► {Applied | Failed | DispatchUncertain}`.
  `flushable` becomes **one pure function** `fn flushable(&self, clock) -> bool`
  consumed by *both* the SQL prefilter and the in-Rust gate, so they cannot
  disagree. "Held" becomes visible to the UI (a truthful "scheduled / will send
  when open") instead of an invisible Pending-that-isn't-flushable.

- **D152 — Un-fuse the `send_at` mechanism (the P0 fix).** The "one send_at hold"
  unification fused two different temporal contracts under one absolute timestamp
  and one frozen clock that serves neither. Split them, and require the SAME clock
  to stamp and to judge:
  - **Undo-send (relative):** deadline = **enqueue instant + delay**, measured in
    monotonic *elapsed*, computed **server-side** — not a client wall-clock
    absolute. Suspend during the window just makes it due on wake (correct: the
    user was away). Never early, never unboundedly late.
  - **Send-later (absolute wall time):** fire when a **re-sampled** `SystemTime::now()`
    ≥ target, with the monotonic anchor used only as a floor against a backward
    step. Tracks real time; never stuck.

- **D153 — `DraftRegistry` is the sole identity authority.** One owned lifecycle
  replaces the four seams:
  `reserve(account,key)->DraftRef · rotate(account,key,provider_id) · forget(account,key) · resolve(account,key)->Option<DraftRef>`.
  `resolve` miss is a **typed outcome**, never a silent guess. Delete
  `resolve_draft_flush_target`'s `unwrap_or_else(key)` fallback, the
  `draft_message_exists` projection probe, and the alias / `message.draft_id` dual
  source. Registry write-through happens in the **same store transaction** as the
  canonical message write (the M69 intent — enforced; no second runtime-side write).

- **D154 — Complete the send-outcome space; kill `moved_to_sent`.** Replace the
  warn-and-return with a total outcome:
  `SendOutcome = Delivered { filed: Filed | PendingFiling } | Uncertain(reason) | Failed(reason)`.
  `Delivered { PendingFiling }` (== `moved_to_sent == false`) is a real state that
  (a) surfaces truthfully ("Sent — filing"), (b) carries a **reconciliation
  obligation** the next sync discharges (converge Drafts→Sent), (c) never blocks or
  duplicates. Instantiates DESIGN-L2-test-taxonomy cells `S-CONV-2` / `S-VERD-3`.

- **D155 — Typed + versioned persistence.** Persist states via a **total,
  versioned** mapping; drop the lossy `"conflicted" → Pending` fudge
  (`crates/posthaste-store/src/outbox.rs` parse) and migrate those rows
  explicitly. Requires the SQLite schema-versioning guardrail (currently 🔴 open).

## 4. Migration steps (phased)

Per the owner decision there is **no standalone Phase 0 hotfix**; M80 is the first
step of Phase 1 and is sequenced to land first so nightly send is restored early.

**Phase 1 — restore send + collapse the scheduling shadow (D151, D152, D155)**
- **M80 — Clock un-fusing (restores sending).** Implement D152: relative-elapsed
  undo-send computed server-side; live wall clock for send-later; same clock
  stamps and judges. Regression test with realistic skew (a `now+10s` hold against
  a suspend-lagged monotonic anchor — the exact gap the year-margin tests miss).
- **M81 — Scheduling-as-state.** Implement D151: the `Scheduled { due }` state, the
  single `flushable(clock)` predicate, removal of the SQL shadow gate, and the
  truthful "scheduled" UI surface.

**Phase 2 — identity owner (D153)**
- **M82 — `DraftRegistry` sole-owner.** Collapse the four seams to the one
  lifecycle; delete the fallbacks/probes; enforce single-transaction write-through.

**Phase 3 — send outcomes (D154)**
- **M83 — Complete `SendOutcome`.** Kill the `moved_to_sent` warn; add
  `Delivered { PendingFiling }` + its reconciliation obligation; wire the truthful
  verdict to the UI.

**Cross-cutting**
- **M84 — Schema-versioning prerequisite (gates M81–M83's persistence changes).**
  `PRAGMA user_version` + a migration runner (the parked guardrail, now with a
  concrete driver). Drop/migrate the `"conflicted"` legacy state.
- **M85 — Taxonomy L2 fault-tests per phase.** Each M lands with the SEND-grid
  cells it satisfies (M80→S-VERD-4/liveness; M83→S-CONV-2/S-VERD-3; etc.), using
  the L2 fault-injection seam (DESIGN-L2-test-taxonomy §5.2).

## 5. Convergences

- **Pulls in the schema-versioning guardrail** (M84) — the parked AUDIT item now
  has a real driver.
- **Instantiates the test taxonomy** — the `SendOutcome` states map 1:1 onto the
  SEND grid's convergence/verdict cells.
- **Retires the `moved_to_sent` smell** flagged across AUDIT-L2-architecture-health
  (S2 canonical write-through) and BETA-READINESS (DS6, 🟠 PARTIAL).

## 6. Open questions

- **`Scheduled` as a distinct state vs. a typed `Schedule` field on the op.** A
  distinct state is cleaner for the UI and the transition validator; a typed field
  keeps the state enum small. D151 leans distinct-state — confirm.
- **Undo-send relative-elapsed vs. absolute across a client/daemon split.** In
  split (remote authority) mode, "enqueue instant" is the daemon's — is that the
  desired anchor for a remote client's undo window? (Bundled app: same machine,
  no issue.)
- **`DispatchUncertain` currently fuses "post-write timeout" and "crashed
  inflight."** Should M83 split them, or keep fused? (They differ in whether a
  submission definitely left the socket.)
- **RECOVER vs CONVERGE** (carried from the taxonomy) affects whether
  `PendingFiling` reconciliation is modeled as its own obligation or as a
  CONVERGE cell.

## 7. Status / next

DESIGN, not ratified. On ratification: implement Phase 1 (M80 first — restores
nightly send), then M84 (versioning) gating Phases 2–3. Nothing is implemented
under this RFC yet.
