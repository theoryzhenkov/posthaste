---
scope: L2
summary: "Redesign of local-first mutations: one overlay, idempotent fold, version-based convergence, single optimistic source — replacing the accreted flush/reconcile special cases"
modified: 2026-06-22
reviewed: 2026-06-22
lifecycle: ephemeral
type: DESIGN
depends:
  - path: docs/stale/L1-outbox
  - path: docs/stale/L1-sync
  - path: docs/runtime/L2
  - path: docs/backend/L1
  - path: docs/client/L1
  - path: docs/eph/DESIGN-L2-account-state-architecture
---

# Local-first mutations: diagnosis and redesign

> **Realized 2026-06-22 as [`docs/replication/L1`](../replication/L1.md).** This
> doc is the precursor diagnosis; the permanent mechanism (coherent links: the
> optimistic up-channel, authoritative down-channel, and confirmation-watermark
> convergence, composed across client/runtime/backend) now lives in the
> replication domain, with revised `runtime/L1`, `client/L1`, `backend/L1`,
> `api/L1`, and `state/mail/L1`.

This is subsystem **(1)** of the three-owner redesign (mutations/read-model;
accounts/runtime — see [[DESIGN-L2-account-state-architecture]]; UI surface/app-mode).
It supersedes the reactive `docs/stale/L1-outbox` spec, which itself accreted
edge cases (`reconcile-after-automation`, flush-ordering pair, stale-inflight,
legacy-`conflicted`) and is the source of the visible flicker.

## 1. Symptoms

- **Flicker on a mutation** (toggle flag, archive): the row shows the new state,
  then snaps back to the old state, then settles on the new state.
- Downstream UI consequence (owned by subsystem 3, not fixed here): the
  post-mutation invalidation storm transiently empties the `accounts` query and
  trips `shouldForceAccountSettings`, yanking the user into Settings.

## 2. Root cause: five uncoordinated truths for one displayed value

A single message's displayed keywords/mailboxes are written or patched in **five**
independent places, settling in an uncoordinated order:

| # | Source | Owner | When it changes the view |
| - | --- | --- | --- |
| 1 | `message` projection + memberships (SQLite) | sync writer | after a sync batch commits |
| 2 | outbox **overlay** (pending/inflight assertions folded on read) | `message_queries::fold_message_overlay` | at read time, while ops are non-terminal |
| 3 | client react-query cache (base read models) | query client | on fetch / invalidate-refetch |
| 4 | client **optimistic projection + undo** (`operations.ts` `MutableState`, `useOperations.run`) | web client | immediately on the action, reverted on undo |
| 5 | client **keyword event patch** (`applyKeywordEventPatch`) | `domain-cache/handlers` | on each `message.updated` SSE |

The flip is mechanical:

1. (4) patches the client cache → row shows new state instantly.
2. The command enqueues an op; backend (2) also shows new state on the next read.
3. Flush succeeds → **the op is pruned immediately** (`flush_account` →
   `remove_operation`), so (2) disappears **before** (1) is updated.
4. `operation.settled` / `sync.completed` invalidate the read models; (3)
   refetches the **base projection**, which has not yet been updated by a
   converging sync → row shows the **old** state.
5. A later sync updates (1); (5) and refetch finally show the new state.

The window in step 3–4 — overlay retired before projection converges — is the
flicker.

## 3. Root cause: convergence is coordinated by hand at every call site

The projection (source 1) is written only by sync; the overlay (source 2) is
retired on flush success. Nothing ties "flush succeeded" to "projection now
reflects it," so each call site stitches the two together differently:

- flush **before** a pull sync, and flush **again after** sync;
- do **not** advance the local message cursor on assertion flush (to force the
  next sync to re-observe);
- `reconcile-after-automation`: an extra observation after automation flush;
- `backfill` flush then reconcile;
- legacy `conflicted` recovery; stale-`inflight` recovery; send-once.

Each is correct in isolation; together they are an un-specifiable web with no
single invariant to extend. New surface (automation, backfill, the next feature)
⇒ new bespoke stitch. This is the fragility called out for redesign.

## 4. Target invariants

1. **One base, one overlay, one fold.** A displayed entity is
   `fold(projection_base, overlay_ops)` computed in exactly one function, used by
   every read (list, detail, counts, smart mailbox) **and** by the client. The
   base is *only ever* server truth. Nothing else patches a displayed value.
2. **The projection is written only by sync.** Mutations never write the
   projection and never advance the stored sync cursor.
3. **Assertions are idempotent states, not deltas of record.** `setKeywords`,
   `replaceMailboxes`, `destroy` carry a desired state; folding one over a base
   that already reflects it is a no-op. (Already true of the fold; we now *rely*
   on it.)
4. **Retire on convergence, not on flush.** An applied op stays in the overlay
   until a sync has provably observed its effect into the projection. Because the
   fold is idempotent (inv. 3), the overlap — base updated **and** op still
   folded — is visually harmless, so the exact retire instant cannot flicker.
5. **One lifecycle for every kind.** Enqueue → fold → flush → observe → retire is
   identical for messages, drafts, and sends. The *only* kind-specific code is
   the provider push mapping and whether the kind mints an entity.

Invariant 4 is the load-bearing change. Today's flicker is a direct violation of
it (retire happens at flush, before convergence); today's special cases all exist
because invariant 4 was never stated, so convergence was stitched per call site.

## 5. Target design

### 5.1 Operation lifecycle (one state machine)

```
        enqueue
          │
          ▼
       pending ──flush push ok──▶ applied ──observe convergence──▶ retired (removed)
          │                         │
   transient (offline)        permanent reject
          │                         │
          ▼                         ▼
       pending                    failed
```

- `applied` is now a **resting, overlay-visible** state ("provider accepted it;
  projection not yet caught up"), *not* an immediate prune. This is the change
  that closes the flicker window.
- `retired` = removed from the overlay. Happens only at a convergence observation.

### 5.2 The convergence rule (replaces every stitch)

Because mutations never advance the stored sync cursor (inv. 2), the stored
cursor always **predates** every applied op's change. Therefore a single ordinary
incremental sync from the stored cursor is *guaranteed by the provider* to carry
the delta for every applied op. So convergence is one unconditional cycle:

> **flush → observe → retire.** After flushing, run one incremental observation
> sync; on its successful commit, retire every op that was `applied` before the
> observation began (atomic with the cursor advance).

This one cycle is invoked identically by user mutations, post-sync automation,
and backfill. `reconcile-after-automation`, the flush-before/after-sync
asymmetry, and "don't advance the cursor" all collapse into it — they were
hand-rolled instances of this rule. (Implementation note: retire eligibility is a
captured marker — the set of op ids `applied` at observe-start, or an applied-seq
watermark — not a comparison of opaque provider state tokens, which are not
ordered.)

### 5.3 One fold, one read path

`fold(base, ops)` already exists as `apply_operations_to_summary`. Finish
unifying it:

- Counts use it unconditionally (drop the SQL fast-path vs overlay split in
  `mailbox_count_overlay` / smart-mailbox counts — fold is O(pending), and
  pending is empty in the common case, so the fast path is premature).
- The client uses the **same** fold over `(base read model, listPendingOperations)`
  rather than its own patch. The fold logic is shared (ported to TS or exposed as
  a runtime read that returns the already-folded view — see options below).

### 5.4 Single optimistic source on the client

Delete the client's parallel optimism (source 4) and the keyword event patch
(source 5). A mutation becomes: **enqueue (local, synchronous on the daemon) →
the overlay now includes it → the view re-folds.** Instant feedback comes from
the overlay, not a separate cache patch. `operation.settled` no longer drives a
view change at all (the base+overlay already agree); it only surfaces *failures*.

Undo stops being a client state machine: **undo = enqueue the inverse assertion**,
which coalesces with the original per the existing coalescing rules. The
before/after capture in `operations.ts` is replaced by "project current overlay
state to its inverse and enqueue it." `destroy` remains the one non-invertible
kind.

## 6. Options on the two genuinely open choices

The three high-level decisions (single overlay; version/convergence-based retire;
snapshot status) are locked. Two implementation choices remain:

### 6.1 How the client gets the folded view

- **A — Client folds (recommended).** Cache stores server base; client folds
  `listPendingOperations` over it in a selector using shared fold logic. Pro:
  instant, offline-correct, one cache entry per read, no refetch on mutate. Con:
  fold logic must exist in TS (port) or be WASM-shared.
- **B — Server returns folded view + push deltas.** Mutation/reads return
  already-folded models; client stores them verbatim. Pro: single fold
  implementation (Rust). Con: every mutation needs a round-trip to look right;
  reintroduces the latency the overlay exists to hide; harder offline.
- **C — Keep client patch, tag by op id, retire on convergence event.** Pro:
  smallest diff. Con: keeps two optimism engines (the thing we are removing);
  still races unless convergence events are perfectly ordered.

Recommend **A**: it is the only option that is both instant and single-source.
The TS fold is small (three assertion kinds) and can be covered by the same
assertion tests as the Rust fold.

### 6.2 Convergence observation granularity

- **Coarse (recommended): retire on the next full observe-commit.** Simple, one
  marker. Safe because idempotent fold tolerates the overlap. An op lingers in
  the overlay at most one sync cycle longer than strictly necessary — invisible.
- **Fine: per-op provider-cursor compare.** Retire each op the instant its exact
  change is observed. More precise, but requires ordering opaque provider tokens
  (JMAP state strings, IMAP MODSEQ) which are not uniformly comparable, and buys
  nothing the user can see.

Recommend **coarse**.

## 7. What this deletes

Directly addressing the "lengthening special-case functions" concern, the target
removes or collapses:

- `flush_account_and_reconcile` (the post-automation special) → folds into the
  one `flush → observe → retire` cycle.
- The flush-before-sync / flush-after-sync asymmetry → one cycle.
- "Do not advance the local cursor on assertion flush" → unnecessary; mutations
  never touch the cursor by rule.
- The SQL-fast-path vs overlay split in counts → one fold.
- Client `operations.ts` optimistic projection + `useOperations` rollback +
  `applyKeywordEventPatch` → one client fold + inverse-op enqueue for undo.
- Immediate `remove_operation` on flush success → retire on convergence.

Remaining legitimately kind-specific code (not special-casing, irreducible
provider semantics): `push_operation`'s per-kind provider call, and "only
`draftCreate` mints an entity." These stay, isolated.

## 8. Sequencing

1. **Spec.** Promote this into a first-class `L1-mutations` (interfaces,
   lifecycle, the convergence rule, assertions) at the rigor of `backend/L1`.
2. **Backend lifecycle.** Add the resting `applied` state + the `flush → observe
   → retire` cycle; make convergence retire ops; delete the stitches. Behind the
   existing tests (they already assert overlay reads, coalescing, drafts, send).
3. **One fold everywhere** (counts included).
4. **Client single-source.** Port the fold; drop sources 4 and 5; undo via
   inverse-op enqueue.
5. **Delete** the superseded functions and the stale spec.

Each step is independently testable and shippable; the flicker closes at step 2–4.

## 9. Assertions

| ID | Sev. | Assertion |
| --- | --- | --- |
| one-fold | MUST | Every read (list, detail, counts, smart mailbox) and the client derive displayed state from the single `fold(base, ops)`; nothing else patches a displayed value. |
| projection-sync-only | MUST | Mutations never write the projection and never advance the stored sync cursor. |
| applied-resting | MUST | A flushed op rests in `applied` and remains folded until a convergence observation retires it; it is not removed on flush success. |
| idempotent-fold | MUST | Folding an `applied` op over a base that already reflects it is a no-op (no visible change). |
| convergence-cycle | MUST | Convergence is the single `flush → observe → retire` cycle, invoked identically by user mutation, automation, and backfill. |
| uniform-lifecycle | MUST | `enqueue → fold → flush → observe → retire` is identical across message, draft, and send kinds; only provider push and entity-minting differ. |
| undo-inverse | SHOULD | Undo enqueues the inverse assertion (coalescing with the original) rather than running a separate client rollback engine. |
| no-flip | MUST | A mutation never shows new → old → new; the displayed value is monotonic from the user's action to convergence. |

## 10. Open questions for review

1. **Client fold port (6.1-A)** vs a WASM-shared fold — acceptable to maintain a
   small TS fold covered by shared assertion tests, or do you want one binary?
2. **Undo scope.** Inverse-op enqueue covers keyword/mailbox/role. Confirm
   `destroy` stays non-invertible (no provider-side undelete guarantee).
3. **`applied` durability.** The resting `applied` overlay must survive restart
   (it already lives in SQLite `outbox_operation`). Confirm we also move
   `outbox_operation`/`draft_alias` out of the rebuildable `mail.sqlite` so a DB
   rebuild/quarantine can't drop durable intent (noted previously, still open).
4. **Convergence trigger when fully offline.** With no connectivity, ops rest in
   `applied`? No — offline means flush yields `transient`, so they rest in
   `pending` and the overlay holds; convergence runs when connectivity returns.
   Confirm this is the intended offline read story (overlay is the offline view).
