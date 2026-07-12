# RFC-L2-send-draft-state-machine — mail operations as intents (NS2)

> **Status (2026-07-11): RATIFIED IN SHAPE (owner) — implementation started at
> Slice 0.** REWRITTEN post-NS1 from the original shallow plan ("collapse the
> shadow state machines", 2026-07-10) into the foundational form the owner
> requested: mail operations become **typed intents with effects-as-data**, on
> the NS1 substrate (base sealed to sync, overlay plane, effective reads,
> confirmation-gated retire — RFC-L2-client-replication-model, NS1 COMPLETE).
> This RFC is that parent's **NS2**.
>
> **The nightly P0 (nothing sends) is STILL LIVE until Slice 1 lands.**
> Interim mitigation: set the undo-send delay to 0.
>
> Supersedes/extends: RFC-L2-drafts (D125–D127), RFC-L2-draft-identity
> (D135–D141), RFC-L2-provider-reliability D81–D87 (send-exactly-once).
> Pairs with DESIGN-L2-test-taxonomy (the SEND grid — Slice 5 lands its cells).

## 1. The live P0 (unchanged evidence, fixed by Slice 1)

Every default send is a *held* send whose release clock can only run slow:
the client stamps `sendAt = Date.now() + delay` (browser wall clock,
`useComposeSubmission.ts:260-272`); the flush gate judges it against
`outbox_now_rfc3339()` — anchored ONCE at daemon start, advanced only by
monotonic elapsed (`schedule.rs:66-71`), which **pauses across OS suspend** and
therefore only ever lags. Stamp and judge are different clocks; `send_at <= now`
can stay false forever; the 5s scheduled-send tick consults the same lagging
clock. Tests used year-2020/2999 margins, so realistic skew was never exercised.

## 2. The intent model

### 2.1 An intent has two outputs (D171)

```
Intent ──┬── fold_effects(phase)  → what becomes VISIBLE (rows appear/vanish)
         └── execution plan       → what happens AT THE PROVIDER (steps, ids)
```

**The boundary rule (D171):** *fold effects may depend only on the gesture +
the replicated visible plane; materialization may additionally read
authority-private state (draft registry, provider coordinates) — and decides
only the execution plan, never the visible effect.*

Fold effects live in `replica-core` (the shared kernel), so **client, runtime,
and authority compute the identical prediction from one implementation** — the
authority's overlay fold IS the same function, run at the one node that also
derives the provider plan.

### 2.2 Effects are total (D172)

`fold_effects(Send{key, content}) = [Tombstone(key), Upsert(provisional_sent)]`
**unconditionally** — folding a tombstone over a nonexistent row is a no-op, so
the client's prediction is never visually wrong even when it cannot know
whether a draft exists. Effects are *phase-aware*: a HELD send folds as a
draft-form row (honest: still cancelable), flipping to tombstone+sent at
settlement — a flip that needs no new trigger (settlement already re-derives
the overlay).

### 2.3 Materialization is an authority-side admission step (D170)

The client sends a **raw gesture** — `Send { compose_key, content,
undo_window }` — resolving nothing. At admission (after the apply-ledger), the
authority **materializes**: it consults the draft registry for `compose_key`
and decides what this send *means* (consume-a-draft vs plain send; execution
steps). One resolver, at the one node with authoritative state — the client's
stale view is never load-bearing (the root fix for the draft-identity bug
family; today's immediate-send path doesn't even pass a draftId, so a raced
autosave's draft leaks). Admission is serialized and SaveDraft **reserves its
registry key at admission**, so materialization is strictly better informed
than any client.

### 2.4 Three nodes, one recursive invariant

`visible = fold(base, pending_intents)` holds at EVERY node, with "base" =
what its upstream serves: client (base = runtime's frames; pending = not yet
receipted) → runtime (base = authority assertions; pending = forwarded, not
yet echoed; plus link idempotency admission) → authority (base = provider
truth, compile-sealed; pending = the outbox, folded in the overlay).
Each hop's optimism covers exactly its in-flight window and retires on
absorption. **Client predicts, runtime predicts-and-relays, authority
materializes, sync confirms.** The runtime gains NO materialization logic in
NS2 — only the shared vocabulary extension it inherits from replica-core.

### 2.5 Readiness (D152 revised)

**Readiness is the earliest moment the irreversible provider action may run —
nothing else.** It gates neither folds nor admission nor preparatory steps.

```
Readiness = None | Undo { not_before_mono } | At { wall_rfc3339 }
```

The client sends a **duration** (`undoWindowSeconds`); the **server** stamps
`not_before_mono` on its own anchored clock and judges it on that same clock —
stamp and judge share the anchor, so suspend/NTP skew cancels (relative holds
are exactly what a monotonic anchor is right for). Send-later (`At`) is judged
against a **re-sampled wall clock**. One pure
`flushable(state, readiness, clock)` predicate serves the SQL prefilter and
the in-Rust gate; the two-clocks bug becomes unrepresentable.

### 2.6 Held sends: one row, two-step plan (D173)

Cross-device visibility is required (owner ruling), and settlement-time
fan-out is forbidden, so a held `Send` materializes into ONE outbox row with a
deterministic two-step execution plan decided at admission:

1. **ensure-draft** (eager, NOT held, idempotent by intent id): the draft is
   written to the provider immediately — during the undo window the message is
   a real provider draft, visible and editable on every device.
2. **submit** (readiness-gated): the irreversible dispatch.
   `DispatchUncertain` semantics apply to this step only.

**Undo = cancel step 2** (the existing cancel-vs-flush single-winner gate);
the provider draft simply remains. No client persist-then-schedule two-step,
no `depends_on` pair, no demotion machinery.

### 2.7 The send/draft matrix

| Path | Materialization | During hold | On dispatch |
|---|---|---|---|
| Immediate, no registry draft | plain send | — | Upsert(provisional sent) |
| Immediate, registry has draft | send-consuming | — | Tombstone(draft) + Upsert(sent) |
| Held (undo / send-later) | ensure-draft + held submit | provider draft, cross-device | Tombstone(draft) + Upsert(sent) |
| Undo | cancel submit step | draft persists | — |

## 3. Decisions

Carried from the 2026-07-10 draft (D150–D155, two revised), plus the NS2
design round (D170–D175):

- **D150 — One owning model per lifecycle concern; no shadow state.** (kept)
- **D151 (REVISED) — Scheduling is a typed `Readiness` field + a derived
  `effective_status(clock)`, NOT a state variant.** Execution phase and
  scheduling are orthogonal; `OperationState` stays the minimal Copy enum with
  its transition validator. The anti-shadow win is the single derived
  predicate, not a new variant.
- **D152 (REVISED) — Un-fuse the clock via `Readiness` (§2.5).** The client
  sends durations, never timestamps; the server stamps and judges on one clock
  per readiness kind.
- **D153 — `DraftRegistry` is the sole identity authority**
  (reserve-at-admission / rotate / forget / resolve; typed resolve misses;
  the four seams, `unwrap_or_else(key)` fallback and `draft_message_exists`
  probes deleted). (kept)
- **D154 — Complete the send-outcome space; kill `moved_to_sent`.**
  `SendOutcome = Delivered { filed: Filed | PendingFiling } |
  Uncertain(cause: UncertainCause) | Failed(reason)`. `PendingFiling` keeps the
  provisional Sent overlay row (confirmation-gated — machinery exists) and
  reads "Sent — filing"; `UncertainCause` is a typed field
  (PostWriteTimeout | CrashedInflight | …), NOT extra states. (kept, sharpened)
- **D155 — Typed + versioned persistence.** The intent payload is stored as a
  versioned envelope; the lossy `"conflicted" → Pending` parse fudge is
  migrated away. Gated on M84 (Slice 0). (kept)
- **D170 — Materialization is an authority-side admission step** (§2.3).
  Client intents are unresolved gestures; the client fold is a non-load-bearing
  prediction; convergence rides the existing echo/receipt/absorption paths.
- **D171 — The two-output intent + the boundary rule** (§2.1).
- **D172 — Fold effects are total and phase-aware** (§2.2); written once in
  replica-core for all three nodes.
- **D173 — Held sends are one row with a two-step execution plan** (§2.6);
  eager ensure-draft for cross-device visibility; readiness gates only the
  irreversible step; undo cancels the submit and keeps the draft.
- **D174 — Same-key draft saves COALESCE; `depends_on` chains are deleted.**
  A queued unsent save is replaced by a newer save on the same key
  (last-writer-wins per compose session — the semantics real autosave wants).
  *Adopted by default; reversible if provider-side draft version history turns
  out to matter.*
- **D175 — Lingering-draft self-repair (bounded).** If a send settled
  Delivered but reconciliation later observes the consumed draft still in base
  (e.g. IMAP expunge failed), enqueue ONE provider cleanup delete. *Adopted by
  default over wait-for-sync-only (which would leak the provider copy behind a
  permanent tombstone).*

## 4. Slices (each lands green and whole)

| Slice | Old M | Delivers / deletes |
|---|---|---|
| **0** | M84 | **Schema versioning**: `PRAGMA user_version` + ordered migration runner + downgrade guard + legacy-open fixture test. Migration v1 retires the open-time counter-trigger DROP and drops the dead mailbox counter columns. Policy: additive evolution stays idempotent (`ensure_column`); destructive/transformative changes are versioned migrations. |
| **1** | M80 | **The clock fix** (restores nightly send): `Readiness` split on the current op shape; client sends `undoWindowSeconds`; regression test with a suspend-skewed anchor. |
| **2** | M81/M85 | **Typed intents + effects-as-data**: `MailIntent` typed enum, versioned envelope, `fold_effects()` in replica-core, one effect interpreter in `refresh_message_overlay`. Rider: attempt cap/backoff/quarantine on the flush loop (closes BE-H2, the last open audit HIGH). |
| **3** | M82 | **Draft intents**: SaveDraft (Upsert effect — instant drafts, kills the 10–15s lag / CL-H3 / D132), DiscardDraft (Tombstone effect — **deletes the last `BaseWrite::legacy` production grant**), registry sole authority, coalescing replaces chains (D174). |
| **4** | M83 | **Send as one intent**: unconditional multi-row effects, materialization (D170), two-step held plan (D173), gateway-owned provider consumption (the `DraftDelete` settlement fan-out DELETED), `SendOutcome` (D154 — `moved_to_sent` deleted), reconcile-by-intent-id (+ adopt-by-header for non-dedup providers, closes S-EO-2/M72), D175 repair. |
| **5** | — | **Verdict surfacing + tests**: client undo-send API change, "Sent — filing"/needs-attention states, DESIGN-L2-test-taxonomy SEND-grid L2 cells (S-CONV-2, S-VERD-2/3/4, S-EO-1/2, S-ISO-1) via the L2 fault seam. |

What does NOT change: the outbox exactly-once claim gate, DispatchUncertain
parking discipline (D86), the apply-ledger, the link/receipt/frame flow, the
client entity store's ingestion paths, and the NS1 base/overlay/effective
substrate this all stands on.

## 5. Open items

- Slice-5 UX copy for `PendingFiling` / parked sends (owner review at Slice 5).
- Whether the FTS gap for held-send draft bodies (overlay-only until ensure-draft
  flushes) needs a note in search docs — likely moot since ensure-draft is eager.

## 6. Status

**Slice 1 LANDED (2026-07-11) — NIGHTLY SEND RESTORED.** The two-clock
readiness split (D152): undo holds are duration-stamped and judged on the
daemon's monotonic clock (`hold_until_mono`, new column + partial index);
send-later stays wall-judged against a RE-SAMPLED wall clock; the client's
`sendAt` degrades to display metadata for undo holds and is not stored.
Regression tests pin the P0 shape (a hold with a light-years-skewed client
`sendAt` releases exactly at the mono deadline, both skew directions).
`undoWindowSeconds` added to the wire (openapi + web types regenerated); web
sends it through the schedule path. Full `Readiness` typing folds into
Slice 2's intent envelope.

**Slice 2 rider LANDED (2026-07-11):** BE-H2 head-of-line guard — after
`TRANSIENT_STOP_THRESHOLD` consecutive transient failures an op is skipped
(still pending/retryable/cancelable) instead of halting the drain;
deliberately no permanent quarantine (offline-safety). The last open audit
HIGH is closed.

**Slice 2 CORE LANDED (2026-07-11):** `MailIntent` (domain-model) is the one
decode boundary (`from_parts(kind, version, payload)`); the D155 envelope
version column exists (v1 = historical shapes; unknown versions refused);
every scattered payload reader (flush dispatch, the fold's assertion
extraction — now `intent_fold_effect`, THE effect interpreter — echo
building, send-consume, the DraftDelete flag) matches typed intents;
migration v2 rewrote legacy `conflicted` rows and the parser fudge is gone.
**Deviation noted:** the interpreter lives in domain-service, not
replica-core — `MessageRecord`/command types cannot cross into the wasm-pure
kernel; the shared-kernel effect vocabulary extension ships with Slices 3/4's
multi-row effects, which is when the client prediction needs it.

**Slice 3 LANDED (2026-07-12) — the draft plane folds; the base seal is
total.** `intent_fold_effect` returns the typed `FoldEffect`
(`Assert`/`UpsertDraft`/`TombstoneDraft`; `Send` still `None` until Slice 4)
and `refresh_message_overlay` — now a `MailService` method (every caller had
`&self`; the free-fn form was residue) — folds draft intents keyed by their
registry-resolved LIVE id:

- **Instant drafts (CL-H3/D132 dead):** a queued save IS a visible Drafts
  row (synthesized from the request; `$draft`+`$seen`; sorts by the op's
  `updated_at`), with a projection echo published from the effective read.
  `get_draft_content` serves a still-queued save's content from the op
  payload — offline compose resume can no longer lose the body.
- **Discard = tombstone fold:** supersedes queued saves, skips the provider
  entirely for never-flushed drafts, surfaces `NotFound` per D133 —
  and never writes base. The last `BaseWrite::legacy` production grant AND
  the whole `MessageCommandStore` port are deleted; sync's reconciler is the
  only production base writer. Send-consume (D126) now also folds the
  tombstone at enqueue, so the consumed draft leaves Drafts at settlement.
- **Rotation carry:** at save settlement the old live row is
  hidden/dropped (+ prune echo) and the settled fold is pinned at the
  assigned id (+ projection echo); the post-sync sweep retires it once base
  covers it. Draft entries confirm keywords modulo `$seen` (IMAP appends
  `\Draft` only; exact-set compare would linger forever).
- **D174 landed in full:** same-key saves coalesce via a guarded
  pending-only payload swap (races the flush claim, one winner; op id — the
  create idempotency identity — and kind never change); `depends_on` is
  deleted end to end (model field, store column via migration v3, the flush
  dependency gate, `Operation.dependsOn` on the wire).
- **D153 typed misses:** save AND delete reserve the registry mapping at
  admission, so a flush-time resolve miss can only mean confirmed
  destruction — a `DraftUpdate` re-creates (cross-device last-writer-wins),
  a `DiscardDraft` settles as already-done without a provider call.

**Slice 3 deviations/notes for the next session:**
- The `draft_message_exists` projection probe in `save_draft` SURVIVES (it
  is admission-time materialization reading authority-private state — legal
  under D171's boundary rule; needed for headerless drafts resumed by
  provider id). The full D153 "four seams" cleanup rides Slice 4's
  materialization step.
- BE-H2 reorder corner: a discard enqueued behind a transient-failing
  inflight save can flush first once the save is skipped past the
  threshold; the discard then 404s retryably and converges after the save
  settles (resolve-at-flush retargets it). Accepted; noted here so Slice 5's
  L2 cells can pin it.
- `drafts_mailbox_id` resolves via `list_mailboxes` (which derives live
  counts) once per draft refresh — fine at autosave cadence, but a cheap
  role-lookup store method is the obvious optimization if profiling ever
  cares.

**Slice 4 LANDED (2026-07-12) — send is one intent, in four commits
(4a–4d):**

- **4a (D154):** `MailGateway::send_message` returns `SendFiling`
  (`Filed | PendingFiling`); `OperationSettlement.sendFiling` carries it.
  `moved_to_sent`'s warn-and-forget is dead; with `Uncertain` = the D86 park
  and `Failed` = the failed settlement, the outcome space is complete.
- **4b (D170/D172):** admission MATERIALIZES the compose key (known →
  consuming + reserved; unknown → dropped; the web passes `draftId` on every
  send, killing the immediate-path raced-autosave leak). The fold is
  multi-row and phase-aware: a due send folds
  `[Tombstone(consumed draft's live row), Upsert(provisional Sent row)]` at
  queue time — sent mail is in Sent before any provider call; a HELD send
  folds nothing. Undo/park (D125)/permanent-failure all UNWIND the fold with
  echoes (the park re-pins the draft from the send's own content).
  Consumption is GATEWAY-OWNED: JMAP batches the destroy after the
  submission in one request, IMAP expunges after the Sent append; the D126
  settlement fan-out and the flush follow-up re-pass are deleted.
  Reconcile-by-intent-id: `send_identity_token` lives in domain-model (ONE
  derivation for the gateways' `Message-ID` stamp and the sweep's adoption
  match); the provisional Sent row retires with a prune echo once base
  carries the token-prefixed provider copy.
- **4c (D173):** a held send is ONE row with a two-step plan: eager
  ensure-draft each flush pass (the provider draft exists during the hold —
  cross-device visibility) + the readiness-gated submit. NO schema: the
  registry is the step ledger (done = key rotated to a provider id; DS2's
  deterministic create-id makes crash-retry safe). Admission mints/reserves
  a compose key for every held send; a queued save op on the key IS the
  ensure step. Undo cancels the submit; the ensured draft remains.
- **4d (D175):** the sweep repairs a tombstone whose base row survived the
  sync (lost expunge / silent destroy no-op) with ONE idempotent
  notFound-masked delete, gated on no outstanding op — never stacks, safe
  when premature, uniform across discard/consume/destroy tombstones.

**Slice 4 deviations/notes:**
- The shared-kernel `fold_effects()` move into replica-core did NOT ship
  (deviation carried from Slice 2, now explicit): the interpreter +
  synthesizers stay in domain-service. The client's optimism arrives via the
  sub-second projection echoes, so client-side prediction is not
  load-bearing (D170); move the vocabulary into the wasm kernel only when a
  truly offline client-side fold is wanted.
- The JMAP batched consume-destroy assumes the implicit `onSuccessUpdateEmail`
  response precedes the explicit destroy response (documented at the parse
  site; degrades to a warn + PendingFiling misread, never a failure). Pin
  against real Stalwart in Slice 5's L3 pass.
- A RETRIED failed/parked send does not re-fold until settlement (the row
  reappears then) — cosmetic, noted for Slice 5's verdict surfacing.
- Pre-adoption, the provisional Sent row's body read 404s at the provider
  (the row exists only locally); the window is one sync. Slice 5's
  "Sent — filing"/pending copy should account for it.

**REMAINING: Slice 5** — verdict surfacing (client undo-send API, "Sent —
filing"/needs-attention states over `sendFiling` + `dispatch_uncertain`) +
the DESIGN-L2-test-taxonomy SEND-grid L2 cells (S-CONV-2, S-VERD-2/3/4,
S-EO-1/2, S-ISO-1) via the L2 fault seam.

**Slice 0 LANDED (2026-07-11):** `PRAGMA user_version` + the ordered migration
runner + the downgrade guard (`Conflict`, never `Corruption` — a newer database
is refused, not quarantined) in `db/schema.rs::prepare_schema`. Migration v1
retires the counter-trigger open-time DROP and drops the dead mailbox counter
columns. Covered by `tests/schema_migrations.rs`: a synthetically-downgraded
v0 fixture upgrades once and stays functional; fresh opens stamp; the guard
leaves newer databases untouched. Policy documented at `SCHEMA_VERSION`:
additive = idempotent path; destructive/transformative = versioned migration.

(Slice 1 has since landed — the P0 and its mitigation are history.)
