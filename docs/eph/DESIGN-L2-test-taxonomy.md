---
scope: L2
summary: "A layered test taxonomy (unit / hermetic-integration / provider-conformance / e2e) and a per-operation CONTRACT REGISTRY for the mail-critical mutations (send, move, delete, draft-save). Contracts are DERIVED from a 10-class invariant backbone crossed with lifecycle fault points and outcomes — completeness is a property of the derivation grid, not of the remembered bug list. Design only; nothing implemented."
modified: 2026-07-10
reviewed: 2026-07-10
lifecycle: ephemeral
type: DESIGN
depends:
  - path: crates/posthaste-testkit
  - path: crates/posthaste-store/src/tests/prune_floor_guard.rs
  - path: apps/web/e2e
  - path: .github/workflows/ci.yml
  - path: docs/eph/PLAN-L2-testkit-roadmap.md
---

# DESIGN-L2-test-taxonomy — layered coverage + a derived contract registry

> **Status: DESIGN (2026-07-10). Not implemented.** Motivated by a live
> send-reliability incident (a message delivered but left visibly filed in
> Drafts, with the only sent/not-sent signal being the Stalwart server log) and
> by the broader worry that tests are organized by *code module* rather than by
> *operation contract*, so critical invariants (e.g. "the user gets a truthful
> send verdict") are owned by no single test. This doc defines the taxonomy and
> the derivation method, then fully derives the **SEND** grid as the worked
> template. Move / delete / draft grids are **TODO** (§7) — deliberately not
> guessed until the method is ratified on the hardest op.
>
> Relationship to [PLAN-L2-testkit-roadmap]: this is the *contract* layer on top
> of the testkit machinery. P0–P3d (runtime-in-harness, view-settlement recorder,
> declarative fixtures) are the L2/L3 substrate this reuses; P4 (headless driver)
> underpins L4; P5 (proptest) already landed as
> `crates/posthaste-store/src/tests/prune_floor_guard.rs`.

## 1. Why a registry, not more tests

The existing send coverage is real but **shaped by code, not by contract**: the
outbox state machine, draft consumption, the Drafts→Sent move, and the UI blink
are each asserted in a different file, in a different crate, at a different layer.
No single test owns the end-to-end send *contract*, so a send whose provider
filing silently no-ops passes every layer's test while the user is left unable to
tell whether the mail went out.

The fix is to define the **contract for each operation once** as a set of
invariants, then have each layer prove a *different property of the same
contract*, with a registry mapping every invariant to the test(s) that own it.
Coverage becomes a glance; unowned invariants become visible.

## 2. The four layers

"Unit / integration / e2e", with integration split into a hermetic and a
provider-conformance flavor (the important nuance: a fake provider proves the
*logic*, a real Stalwart proves the *provider actually behaves as the fake
assumed*). All four use harnesses that already exist.

| Layer | Proves | Harness | Speed / CI |
|---|---|---|---|
| **L1 — Unit** | The decision is correct in isolation: phase classifier, draft-identity resolver, `fold_effect` vocab, floor-guard oracle | in-crate `#[cfg(test)]`, no I/O | fast · every PR |
| **L2 — Hermetic integration** | The decision wires through the whole Rust flow (enqueue→flush→gateway→settle→event/view) and the right folds/events fire, **including injected failures** | `Harness::with_runtime()` + `create_mock_account` / `GmailImapFixture` (fake IMAP/SMTP) + `harness.settle()`/`watch_view` + fixture wire log | fast, hermetic · every PR |
| **L3 — Provider conformance** | A *real* provider honors the contract (real JMAP/SMTP semantics, real Drafts→Sent filing, no-dup on timeout) | `StalwartFixture::start()` + `create_jmap_account`, same driver API, no browser | slower · gated `POSTHASTE_STALWART_INTEGRATION=1`; required gate per-op as it stabilizes, else soft |
| **L4 — E2E user journey** | The user *sees the truth*: optimistic → settled UI → truthful toast/state, and the failure affordance | Playwright → real `posthaste-server` → real Stalwart (the `just dev web` / overmind chain; e2e injects a minted session token) | slowest · nightly + pre-release |

**Grounding.** The same `send_message` / `save_draft` / `apply(MailOperation::…)`
+ `sync_account` + `open_link_view` driver runs unchanged against mock,
fake-IMAP, and real Stalwart — only the fixture constructor differs
(`create_mock_account` vs `create_gmail_account` vs `create_jmap_account`). L4's
browser→server→Stalwart chain already exists (`tools/dev/overmind`,
`apps/web/e2e/lib/session-token.mjs`). So L2/L3 are "author tests"; L4 is "author
tests + a fixture lib", not "build infrastructure".

## 3. The derivation method (completeness by construction)

A contract assembled from the known-bug list can only ever contain the bugs
someone already found. Instead, derive invariants from the *structure* of what an
optimistic, replicated, token-scoped mutation is. Three orthogonal axes; the
contract is their cross-product, and every empty cell is either an invariant or a
justified N/A — so gaps are reviewable, not forgotten.

**Axis 1 — state planes that must stay coherent:** **U** optimistic/UI (replica
projector view) · **C** canonical durable store · **O** outbox op record (state
machine) · **R** remote/provider truth · **E** settlement/event stream (the
reconciling signal).

**Axis 2 — lifecycle phases (each boundary is a fault point):** intent → **P1**
optimistic apply → **P2** durable enqueue → **P3** dispatch bytes → **P4**
provider executes → **P5** settle + canonical write-through → **P6** reconcile via
sync → **P7** user verdict.

**Axis 3 — outcome:** success · retriable · **uncertain (post-write)** · permanent.

### 3.1 The invariant backbone (10 classes)

These hold for *every* mutation; a per-op contract is these instantiated with the
op's specifics.

| Class | Statement |
|---|---|
| **AUTHZ** | The op is refused without the correctly-scoped capability, cleanly, with no partial effect (token-scoped, per-operation action derivation) |
| **CONVERGE** | Eventually U = C = R for the op's intended effect; no permanent divergence |
| **ATOMIC-LOCAL** | The canonical write-through is atomic — a crash never leaves a half-applied C |
| **EXACTLY-ONCE** | The provider effect happens once across at-least-once delivery, retry, and restart |
| **NO-LOSS** | Nothing leaves C or R except as the op intends *and confirms* (the mail-safety class) |
| **REVERT** | A permanently-failed op reverts U to match C — no orphan optimism |
| **VERDICT** | U's terminal state ∈ {success, uncertain, failed} and equals R's actual outcome class; uncertain is distinctly surfaced with a recovery affordance; never silently wrong |
| **RECOVER** | A dropped E (missed event) is repaired by a later level-triggered reconcile — no permanent stuck state |
| **ISOLATION** | A stuck/failed op never blocks unrelated ops (liveness; no head-of-line) |
| **ORDER** | Concurrent/competing ops on one entity compose without lost updates (serializability) |

**Two decisions vs the first draft:** ISOLATION was split from ORDER (liveness vs
serializability — bundling hid cells); AUTHZ was added (this is a programmable,
token-scoped mail API — clean scoped refusal is first-class, not an afterthought).
**Open question retained:** whether RECOVER should merge into CONVERGE (RECOVER is
"CONVERGE under a lost signal") — kept separate to *force* the E-gap cells explicit.

### 3.2 Contract per op = {10 classes} × {4 outcomes} × {fault points P3–P6}

Each cell resolves to an invariant or an explicit, reasoned N/A. Completeness
review = "are any cells silently empty?" Regression-scars from past incidents
(DP-C1, DS2, …) are demoted to **guard tests attached to an invariant**, so we
keep incident coverage without pretending the incident list *is* the contract.

## 4. Registry artifact & format

- `docs/testing/README.md` — the taxonomy (§2), gating (§6), how to run each layer.
- `docs/testing/contracts/{send,move,delete,draft}.md` — one file per op: the
  contract as an invariant table (the grids in §5 / §7).
- Each row names its **layer(s)**, an owning **test id** (a stable `#[test]` name
  or e2e filename), and a **status mark**. A follow-up CI check asserts every
  invariant row references a test that exists — turning the registry into an
  *enforced* coverage map. That check is the guardrail against invariants going
  unowned again. (Not built here; design only.)

Status marks double as a principled backlog:
✅ compliant + tested · 🟡 happy-path only / surfacing gap · 🔴 **contract cell
whose invariant is not met or not tested**.

## 5. SEND — full contract grid (the worked template)

`OperationKind::Send`; optimistic fold = `Destroy` of the draft row.
**Send fault points:** **P3-pre** crash/error before bytes written · **P4-lost**
bytes written, response lost/timeout (uncertain window) · **P4-nofile**
submission committed but `onSuccessUpdateEmail` no-op (`moved_to_sent==false`) ·
**P5-torn** crash after provider executes, before settle+write-through ·
**P6-mid** a sync interleaves mid-flight.
Legend: **P**rimary / **C**onfirming layer.

| ID | SEND invariant | Cell (outcome × fault) | Layers | Proposed test | Status |
|---|---|---|---|---|---|
| **S-AUTHZ-1** | Send without the `send` capability → refused before optimistic apply or enqueue; zero effect | success (authz gate) | L1(P) · L2(P) | `send_l2_refused_without_send_scope` | 🟡 per-op authz landed, no send-specific test |
| **S-AUTHZ-2** | Capability expiring *after* enqueue doesn't strand an in-flight send (authorized at admission) | uncertain × P4 | L2(P) | `send_l2_expiry_midflight_completes` | 🔴 unspecified |
| **S-CONV-1** | After settle+sync: U=C=R — in Sent, absent from Drafts, all three planes | success | L2(P) · L3(C) · L4(C) | `send_{l2,l3,l4}_converges_to_sent` | ✅ / 🔴 no L4 |
| **S-CONV-2** | Delivered-but-unfiled (`moved_to_sent==false`) still converges Drafts→Sent on later reconcile | success × P4-nofile | L2(P fault) · L3(C) · L4(C) | `send_l2_nofile_reconciles` | 🔴 **live incident** |
| **S-CONV-3** | A parked send that *did* deliver converges to Sent with no duplicate on reconcile | uncertain × P4-lost | L2(P) · L3(C) | `send_l2_parked_but_delivered_reconciles` | 🟡 |
| **S-ATOM-1** | `{draft consumed, Sent recorded}` write-through is crash-atomic — recovery is never {draft gone ∧ Sent absent} nor {both present} | any × P5-torn | L2(P crash-inject) | `send_l2_settle_is_crash_atomic` | 🔴 untested (non-atomic settlement MEDIUM item) |
| **S-EO-1** | Redelivered/retried send → one delivery (deterministic `phsend-` create-id dedups) | uncertain × P4-lost | L1(P) · L2(P) · L3(C) | `send_{l2,l3}_uncertain_no_double_send` | ✅ |
| **S-EO-2** | On providers that DON'T dedup on create-id (RFC-8620) / IMAP APPEND, adopt-by-header prevents a twin | uncertain × P4-lost, non-dedup provider | L2(P fault) · L3(P) | `send_l3_no_createid_dedup_no_twin` | 🔴 open (M72 adopt-by-header un-landed) |
| **S-EO-3** | After restart, durable apply-ledger re-observes the decision → no re-execute | uncertain × restart | L2(P restart-sim) | `send_l2_restart_no_reexecute` | ✅ (DS7) |
| **S-EO-4** | Same client idempotency-key replay → original op, no double | retriable × client-retry | L1(P) · L2(C) | `send_l2_idem_key_collapses` | ✅ |
| **S-NOLOSS-1** | Permanently-failed send must NOT have destroyed the draft (recovery artifact survives) | permanent | L2(P force-permanent) | `send_l2_permanent_keeps_draft` | 🟡 |
| **S-REVERT-1** | Rejected/failed send → optimistic draft-destroy reverts; draft reappears | permanent/rejected | L2(P settle) · L4(C) | `send_l2_reject_returns_draft` | ✅ (DS8) |
| **S-REVERT-2** | Parked (uncertain) send does NOT hard-revert *or* fake-Sent — shows pending, keeps draft for retry | uncertain | L2(P) · L4(C) | `send_l2_park_shows_pending` | 🟡 surfacing |
| **S-VERD-1** | Success → user sees "Sent", no false-Sent flicker | success | L2(P `assert_confirmed`) · L4(P) | `send_l4_toast_sent` | ✅ / 🔴 no L4 |
| **S-VERD-2** | Uncertain → distinct "needs attention / retry", never silent, never fake success | uncertain × P4-lost | L2(P event) · L4(C) | `send_l4_parked_affordance` | 🟡 |
| **S-VERD-3** | `moved_to_sent==false` → truthful verdict, never a silent Drafts ghost + log line | success × P4-nofile | L2(P) · L4(C) | `send_l4_nofile_verdict` | 🔴 **live incident** |
| **S-VERD-4** | Verdict survives process restart — no "sending… forever" | uncertain × restart | L2(P restart) | `send_l2_verdict_survives_restart` | 🔴 open (BE-H3 volatile send-bridge) |
| **S-RECOV-1** | Dropped `message.updated` (Sent echo) → later level-triggered reconcile still files it in Sent | success × P6/E-gap | L2(P drop-echo) · L3(C) | `send_l2_missed_echo_reconciles` | 🟡 (CL-C3 partial) |
| **S-RECOV-2** | Dropped settlement frame → op re-resolves, not stuck Inflight | uncertain × E-gap | L2(P) | `send_l2_missed_settlement_resolves` | 🟡 (BE-H6 partial) |
| **S-ISO-1** | A permanently-poisoned send doesn't block the account's other ops (attempt cap/quarantine) | permanent × poison | L2(P poison+good) | `send_l2_poison_does_not_wedge_outbox` | 🔴 open (BE-H2) |
| **S-ORDER-1** | Edit-then-send on one draft: the send carries the latest saved content (no lost update) | concurrent | L2(P) | `send_l2_send_after_edit_latest` | 🟡 (DP-H10) |

### 5.1 Explicit N/A cells (the discipline — empty by decision)

- **NO-LOSS × retriable × P3-pre** → N/A: nothing reached R; the local fold is owned by REVERT/CONVERGE.
- **ATOMIC-LOCAL × P3** → N/A: no canonical write-through yet; only the durable outbox record (P2).
- **ORDER × cross-entity** → N/A for send: the op targets a single draft.
- **EXACTLY-ONCE × success-no-fault** → N/A: trivially once; only the fault cells are load-bearing.

### 5.2 The L2 fault-injection seam (derived, not guessed)

The SEND grid *demands* these fault hooks on the fake provider / harness — this
list is the spec for the seam:
`drop_onsuccess_move` (S-CONV-2, S-VERD-3) · `lose_response_after_write` (S-EO-1,
S-VERD-2) · `no_createid_dedup` (S-EO-2) · `crash_between_ack_and_settle`
(S-ATOM-1) · `drop_echo` (S-RECOV-1) · `drop_settlement` (S-RECOV-2) · `restart`
(S-EO-3, S-VERD-4) · `force_permanent` (S-NOLOSS-1) · `poison_op` (S-ISO-1).
**Open question:** whether `GmailImapFixture`/the mock already expose any fault
hooks or this is net-new — the largest single build item behind the taxonomy.

### 5.3 Coverage readout

21 live invariants: ~8 ✅, ~7 🟡, ~6 🔴. The red/yellow cluster is entirely in
**failure, convergence, verdict, and isolation** — not the happy path. That is the
"can't trust send" feeling itemized: the happy path is solid; uncertainty/failure
surfacing is under-owned. The live incident is now two named cells (S-CONV-2 /
S-VERD-3) with a defined obligation (converge + truthful verdict), not a vibe.

## 6. Proposed CI gating

- **Every PR:** L1 + L2 (fast, hermetic — where the contract, incl. fault
  injection, is enforced).
- **Required gate, per-op as it stabilizes:** the L3 rows marked C for that op
  (send already lives in the required `send-path-gate`; extend per op).
- **Nightly + pre-release:** L4 (browser → server → Stalwart) + the broader soft
  L3 suite.

## 7. TODO — remaining op grids (not yet derived)

Deliberately not guessed until the method is ratified on SEND. The identical
10-class grid applies; the ops differ mainly in EXACTLY-ONCE, NO-LOSS, and ORDER.

- **MOVE** (`ReplaceMailboxes` / `MoveToMailbox` / `MoveToRole`, foldable): key
  cells — optimistic leave-source, provider membership, count reconcile
  (count-lag), `MoveToRole` absent-role no-op, multi-mailbox move retry no
  COPY-dup (DP-H3), flush-failure revert.
- **DELETE** (`Destroy`, hard/idempotent; distinct from move-to-Trash): key cells
  — optimistic disappear, provider destroy-everywhere, idempotent re-destroy,
  DP-C1 preserve `imap_message_location` pre-flush (regression guard), flush-fail
  revert.
- **DRAFT-SAVE** (`DraftCreate`/`DraftUpdate`, not foldable, reconciles via
  `message.updated`, idempotency key = stable `draft_id`): key cells — create
  appears (known latency), update-in-place no twin, deterministic create-id no
  twin on redelivery (DS2 guard), replace inspects destroy outcome (DS3 guard),
  cross-op key reuse → Conflict (D128).

## 8. Status of the machinery this depends on

- **L1/L2/L3 driver + fixtures:** exist (`posthaste-testkit`; PLAN-L2-testkit-roadmap P0–P3d).
- **L4 chain (browser→server→Stalwart):** exists (`tools/dev/overmind`, `apps/web/e2e/lib`).
- **P5 proptest exemplar:** landed — `crates/posthaste-store/src/tests/prune_floor_guard.rs` (prune/floor-guard invariants).
- **To build (later, on approval):** the L2 fault-injection seam (§5.2), the L4
  e2e fixture lib + per-op cases, the registry docs + the coverage-check CI lint.
