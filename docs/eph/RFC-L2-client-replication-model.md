# RFC-L2-client-replication-model — one writer, one fold, mutations as intents

> **Status (2026-07-11): PROPOSED / DESIGN — not ratified, nothing implemented.**
> The north-star this session converged on. It is the *parent* frame that
> [RFC-L2-send-draft-state-machine] (the send/draft slice) and
> [DESIGN-L2-test-taxonomy] (the coverage contract) both point at. The core
> claim: the client-side mail engine is a **local replica of an authoritative
> upstream**, and nearly every robustness bug we found is one bug —
> *multiple components write the same fact through different paths and reconcile
> by hand.* The fix is a single structural invariant, **enforced by the Rust type
> system (Tier 1)** so it cannot silently regress between agent context resets.
>
> **[Update 2026-07-11]:** §3 verified against code (the runtime read path
> *already* runs the shared replica-core fold; the gap is narrower than first
> stated) and **§6 added — the base/overlay/effective substrate** (D167–D169):
> the concrete mechanism by which the runtime keeps base pure while staying
> SQL-queryable. This reshapes the NS sequence (§7).

## 1. The invariant

```
visible_state  =  fold( base , intent_log )
```

- **`base`** — last-known authoritative upstream truth. **Exactly one writer: the
  reconciler (sync).** Nothing else may write it.
- **`intent_log`** — durable, ordered log of local mutations, each a first-class
  object with a full lifecycle. **Exactly one writer: the command handler.**
- **`fold`** — a pure, deterministic function. Never written directly; the UI
  reads only this.

One writer per plane; everything visible is a pure function of the two. This
single rule dissolves the bug class (counts drift, draft-identity four-seams,
`moved_to_sent`, the send fan-out): there is no sync-vs-settlement-vs-optimistic
reconciliation to get wrong, because the fold *is* the reconciliation.

## 2. Mutations are intents (the single transaction)

A user action produces **one** intent — whole lifecycle, one object, no fan-out:

```
Intent {
  id:        stable, client-generated, == the provider idempotency key
  effect:    a pure transform on the projection (what the fold applies)
  readiness: not_before (a duration for undo; a wall-time for send-later)
  status:    proposed → submitted → (confirmed | rejected)   // derived, never side-written
}
```

Four properties, each of which kills a current bug *structurally*:

- **The id is the reconciliation key.** The client stamps its intent id into the
  provider object (already done: `X-Posthaste-Draft-Id`, deterministic `phsend-`
  create-id). At-least-once delivery is now safe: the authority dedups on the id,
  and the reconciler matches "my intent → this authoritative object" by it. →
  duplicate-send / twin-draft die by construction.
- **Uncertainty is resolved by reconciliation, not by certainty at write time.**
  The client never needs a synchronous "did it land?" — sync observes the truth
  and retires the intent. → `DispatchUncertain` and `moved_to_sent` stop being
  coded/tested branches; they become "submitted, effect not yet observed."
- **Confirmation = the reconciler observing the effect in `base`, then retiring
  the intent.** ("Retire on confirmation", universal.) No settlement writes base.
- **Readiness is a pure predicate over one clock.** No `Scheduled` state, no SQL
  gate, no frozen clock. → the send P0 cannot be expressed. (See
  RFC-L2-send-draft-state-machine D151/D152: schedule is a typed field on the
  intent + a derived status, not a state; undo-send is relative-elapsed, computed
  server-side; send-later is live wall-clock.)

**Send in this model:** one intent `Send(draft_id)`. Its effect (folded instantly)
is "the draft projects as a sent message" — leaves Drafts, appears in Sent, in the
fold. No separate `DraftDelete` op, no "did the server move it" branch. Confirmed
when the reconciler sees an authoritative Sent message carrying `id`. That is the
single send transaction.

## 3. What is already right vs. what violates the model (verified 2026-07-11)

| Piece | Status |
|---|---|
| `replica-core` (the `fold`) | ✅ already the right model |
| `replica-projector` (reactive windowed views) | ✅ already there |
| `OptimisticReplica` trait (the seam) | ✅ named |
| **the runtime read path runs the SAME shared fold** | ✅ **verified** — `runtime/src/read.rs` runs an `AuthorityServerPendingSet` of replica-core `PendingMessageMutation`s over base rows; `near_node.rs:319-322` asserts the kernel is `OptimisticReplica<MessageConvergence>`, "the same seam the client uses". Wasm-sharing of the engine is **realized**, not aspirational |
| **one predicate engine (SQL), by deliberate design** | ✅ **verified** — the rules engine has NO Rust evaluator; `rules/engine.rs:362-378` matches a message by running the indexed SQL query scoped to one message id ("the codebase's predicate path"). Any design that adds a second query evaluator *creates* the dual-engine problem |
| stable/deterministic ids (`phsend-`, draft-id header) | ✅ exist — but used as auxiliary keys, not the *universal* reconciliation key |
| **reconciler as the *sole* base-writer** | ❌ **violated — but narrower than first thought.** The violation is specifically: (a) the S2 canonical write-through (`MessageCommandStore::set_keywords/replace_mailboxes/destroy` materializing optimism INTO base), (b) settlement writing base via `apply_sync_batch` (`outbox/settle.rs:72`), (c) the `protected_message_ids` entanglement that (a) forces onto sync. Note: `domain-model/model/outbox.rs`'s own doc header states "Pending operations are a read-time overlay… **sync remains the only writer of that projection**" — the write-through violated the codebase's *own written contract*. This RFC restores it, not invents it |
| **mutation as a single intent** | ❌ **violated** — send fans out (Send op + follow-up DraftDelete + a later sync that actually files it); draft-lifecycle is separate op-kinds |
| **verdict as a pure projection of intent status** | ❌ **violated** — `moved_to_sent` warn, parked surfacing |

The read path (fold + projector) is already on the model; the work is disciplining
the **write** path to (a) one base-writer and (b) mutations-as-intents. The pieces
are in the box; the invariant isn't enforced. This makes the north-star reachable
as **convergent refactoring**, not a greenfield rewrite.

**Why the write-through exists at all (the crux §6a solves):** SQL can't fold.
The shared fold is per-entity and in-memory, but the runtime must answer queries
the fold can't reach into — rule/smart-mailbox predicates
(`query_message_page_by_rule`), counts, FTS, and the rules engine's
`match_message`. Optimism was therefore **materialized into base** to make those
SQL reads reflect it — and that materialization is exactly what collides with
sync (hence `protected_message_ids`, the M35 guard, the DP-C1 class).

Payoff: under this model most of the DESIGN-L2-test-taxonomy invariant grid
(CONVERGE, EXACTLY-ONCE, NO-LOSS, VERDICT) becomes true *by construction* rather
than verified after the fact.

## 4. Decisions

- **D160 — The invariant is `visible = fold(base, intent_log)`, one writer per
  plane.** Non-negotiable target for the write-path refactor.
- **D161 — The reconciler (sync) is the SOLE writer of `base`.** Settlement and
  the optimistic path stop writing canonical state; the fold + reconciliation
  replace them.
- **D162 — Every mutation is a single intent** with a pure `effect`, a stable
  `id`, a `readiness`, and a derived `status`. No fan-out into follow-up ops.
- **D163 — The intent `id` is the universal reconciliation key**, stamped into the
  provider object; reconciliation matches and retires by it.
- **D164 — The verdict is a pure projection of intent status + base**, never a
  side-channel (no warn-log verdicts).
- **D165 — Enforce D161 in the Rust type system (Tier 1, ACCEPTED).** Base-mutating
  store functions require a `BaseWrite` capability witness whose constructor is
  private to the reconciler module. Any base write from elsewhere is a **compile
  error** — the invariant cannot be silently violated between agent context
  resets. This is the enabling guardrail; it lands *before* any migration.
- **D166 — Tier 2 (external linters) DEFERRED.** dependency-cruiser (TS import
  layering), Semgrep (call-site rules), dylint (Rust custom lints), cargo-deny
  `[bans]` — valuable backstops for the edges the type system can't seal, but not
  now. The type-system seal is the primary and sufficient first mechanism.
- **D167 — Optimism is materialized as the fold's OUTPUT into a separate overlay
  plane, never into base.** A new `message_overlay` table (same schema as
  `message` + a tombstone flag) holds folded rows computed by the *existing
  shared replica-core fold*: accepted op → folded row written to overlay; base
  change touching a pending row → refold (the pending set's existing
  `apply_base`) rewrites the overlay row; retire-on-confirmation → overlay row
  deleted. **SQL contains zero fold semantics** — it only merges two tables.
  Writers: sync → base ONLY; the fold engine → overlay ONLY. (The LSM/memtable
  pattern: base = committed layer, overlay = memtable, reads merge.)
- **D168 — Every SQL read goes through the effective view.** `message_effective`
  = base rows whose id is not in the overlay, `UNION ALL` non-tombstone overlay
  rows. Rule/smart-mailbox queries, counts, `list_messages`, and the rules
  engine's `match_message` all read `_effective`; the predicate applies
  uniformly to both branches, so **SQL stays the single predicate engine**
  (no Rust evaluator is built — see §3). The overlay is bounded by the pending
  outbox (dozens of rows), so the union plans cheaply.
- **D169 — One engine, two storage backends via an `OverlayStore` port.** The
  fold engine (replica-core + pending set + projector — already shared) gains a
  small storage port with two impls: the in-memory maps the client de-facto has,
  and the new SQLite overlay. Per plane and per node:
  base = {client: in-memory entity store fed by link frames | runtime: SQLite
  `message` fed by sync}; overlay = {client: in-memory pending folds | runtime:
  SQLite `message_overlay`}; merge = {client: native in the store | runtime: the
  `_effective` view}. No second engine anywhere.

## 5. Method (why incremental + enforced, not big-bang)

A direct, big-bang re-foundation driven by an agent is the highest-risk way to add
*more* of exactly the agentic patchwork this repo suffers from: the correctness is
a **global, emergent** property (one writer; crash-consistency; idempotency edges)
that the compiler and unit tests can't see, the blast radius is **mail loss**, and
an agent loses cross-step coherence across a long horizon. So:

1. **The type-system seal (D165) makes the global invariant a local, per-commit
   compiler check** — the thing that makes agent-driven migration safe at all.
2. **Tests first** — DESIGN-L2-test-taxonomy's invariant grid + the L2
   fault-injection seam verify correctness-under-failure before touching the
   dangerous paths.
3. **Strangler-fig, one operation at a time** — new intent+reconciler path stood
   up alongside the old; migrate **send** end-to-end first; keep both green;
   retire the old writer. Each step compiles, passes the taxonomy, is reversible.
4. **Human at the load-bearing moments** — the base-writer cutover,
   crash-consistency, provider-idempotency edges get human sign-off, not agent
   judgment.

## 6. The runtime substrate: base / overlay / effective (D167–D169)

```
base plane      SQLite `message` etc.     writer: sync ONLY           [LSM committed layer]
overlay plane   SQLite `message_overlay`  writer: the fold engine     [memtable]
effective view  base ∪ overlay            what every SQL query reads  [merged read]
```

**Overlay lifecycle (= retire-on-confirmation, which already exists in the
pending set — this gives it a second storage target, nothing more):**

1. **accept** — op enqueued → the shared fold computes the folded row (keywords/
   mailboxes replaced; tombstone for Destroy; full row for a draft create from
   the op payload) → written to `message_overlay`.
2. **refold** — sync upserts a base row that has pending effects → the pending
   set's existing `apply_base` refolds → overlay row rewritten. (No flicker:
   the overlay row persists until base has caught up.)
3. **retire** — the reconciler observes the effect in base → op retires →
   overlay row deleted.

**What this deletes:**
- the S2 canonical write-through (`MessageCommandStore` optimistic writes into
  base) → the DP-C1 class (optimistic write clobbering sync-owned state)
  becomes *unrepresentable*;
- `protected_message_ids` and the `apply_sync_batch_protected` sibling — one
  pure `apply_sync_batch` remains, knowing nothing about the outbox; sync
  prunes base freely, unsynced creates live in the overlay it never touches;
- the write-through-vs-sync races and the guard-the-guard machinery (M35
  durable-snapshot guard's outbox entanglement);
- and the `BaseWrite` seal (D165) becomes cheap and honest — sync genuinely IS
  the only base writer, so the capability locks a door instead of inventorying
  violations.

**Honest costs / open edges:**
- *Refold writes:* one overlay rewrite per sync upsert touching a
  pending-affected row. Overlay is small; negligible.
- *Ordering/pagination at page boundaries:* a folded row can move across a page
  under the union. Anchor-based paging + the tiny overlay make this tractable,
  but it needs targeted tests (a DESIGN-L2-test-taxonomy L2 cell).
- *FTS gap:* unsynced draft bodies are not in the FTS index until synced.
  Acceptable; document alongside the existing search-preview caveats.
- *Migration is read-path-by-read-path* (strangler), each step verified by
  differential tests (same query against the old path vs `_effective`).

## 7. Program / sequence (reshaped 2026-07-11)

- **NS1 — Overlay substrate (D167/D168/D169).** Stand up `message_overlay` + the
  `_effective` view + the `OverlayStore` port on the fold engine; move the SQL
  reads over one by one (rule queries, smart mailboxes, counts, `list_messages`,
  rules `match_message`) with differential tests; then delete the S2
  write-through and collapse `apply_sync_batch_protected` into one pure
  `apply_sync_batch`.
- **NS1b — The `BaseWrite` capability seal (D165).** Lands as the *capstone* of
  NS1, when sync is genuinely the sole base writer — the seal then enforces
  reality rather than inventorying violations. (A minimal mechanism-spike of the
  witness pattern may precede NS1 to validate the approach cheaply; owner's
  call at the time.)
- **NS2 — Send as a single intent (D162/D163/D164).** Migrate send onto the
  model: one intent, effect subsumes draft consumption, id-keyed reconciliation,
  verdict as projection. Folds in the send P0 clock fix and kills
  `moved_to_sent`. Nests RFC-L2-send-draft-state-machine M80–M83.
- **NS3 — Generalize** to move / delete / draft-save once the send pattern is
  proven and the seal holds.
- **Cross-cutting** — SQLite schema-versioning (the parked guardrail) gates the
  persistence changes (`message_overlay` is itself a schema change — a concrete
  driver); taxonomy L2 fault-tests land per step.

## 8. Links

- [RFC-L2-send-draft-state-machine] — the send/draft slice (now a child of NS2).
- [DESIGN-L2-test-taxonomy] — the coverage contract; its grid becomes largely
  true-by-construction under D160.
- AUDIT-L2-architecture-health — the parked SQLite schema-versioning guardrail
  (cross-cutting prerequisite for NS2/NS3 persistence).

## 9. Status / next

Ratified by the owner 2026-07-11; implementation started. **NS1 increment 1
LANDED (2026-07-11):** the overlay plane (`message_overlay` +
`message_mailbox_overlay` + `message_keyword_overlay`), the three `_effective`
views (created after `ensure_column` evolution — CREATE VIEW validates
columns, so legacy-DB opens would otherwise break), the `MessageOverlayStore`
port (`ports/overlay_store.rs`) + SQLite impl (`store/src/overlay.rs`), and
the first strangled read family: smart-mailbox/rule queries + counts + the
rules engine's `match_message` + the shared summary hydration all read
`_effective`. The body-FTS predicate was re-keyed from `m.rowid` to
`(account_id, id)` row-value correlation (a view has no rowid; folded rows
stay body-searchable via their base content). Covered by
`store/src/tests/overlay.rs` (merge semantics) + the untouched existing suite
(empty-overlay differential).

**NS1 increment 2 LANDED (2026-07-11):** every remaining client-visible SQL
read strangled onto `_effective` (list/page, thread/conversation,
summary/detail + list_unsubscribe, tags, conversation-by-rule, FTS search via
a base-rowid→effective join). The shared `_tx` helpers stay on BASE
deliberately — they serve the write/sync plane's event scoping and the S2
write-through readbacks, which die with the write-through. **The read-side
sequencing gate is satisfied.**

**NS1 increment 3 LANDED (2026-07-11) — the counts gate is RESOLVED:**
mailbox counts are now a live derivation over the `_effective` plane
(one GROUP BY in `list_mailboxes`); the incremental counter triggers are
retired (dropped at open on legacy DBs), so the DP-H12 drift class is
structurally gone; `mailbox.unread_emails/total_emails` are dead columns
pending M84. Conversation aggregates needed nothing — the wave-2 strangle
already computes them live, and the stored `thread_view`/`conversation`
aggregates have no readers (delete with M84 as dead machinery).

**NS1 increment 4 LANDED (2026-07-11) — THE CUTOVER.** One coherent change:
- Mutations write the OVERLAY, never base: `apply_assertion_to_overlay`
  (mutation.rs) queues the op, re-derives the overlay entry via the single
  lifecycle function `refresh_message_overlay`, and builds the echo event
  from the EFFECTIVE read — echo, lists, and counts are one derivation.
- Settlement writes the RAW readback to base (provider truth via the flush
  channel — the reconciler role) and re-derives the overlay; the old
  fold-remaining-ops-into-base is gone.
- **Retire-on-confirmation is real**: `OverlayRetire::{Immediate,
  ConfirmAgainstBase}` — a blind (no-readback, e.g. IMAP) settlement keeps
  the folded entry until a sync writes the effect into base (found live by
  the real-store `automation_rules` suite; prevents the settle→sync revert
  flicker). Rejections retire immediately (revert now).
- Sync writes raw truth only: `guard_unsettled` DELETED; the
  `_protected` port/impl/param chain DELETED end to end (M35 obviated —
  unsynced optimism never reaches base); post-sync `sweep_message_overlay`
  refolds surviving entries over the fresh base, confirmation-gated.
- S2 write-through call sites in mutation.rs are gone. `MessageCommandStore`
  survives for ONE caller: the draft-discard destroy (`outbox/draft.rs`) —
  an entity-op path that cuts over in NS2 (gets a legacy grant at seal time).

**NS1b LANDED (2026-07-11) — NS1 IS COMPLETE.** The `BaseWrite` capability
witness (D165) seals every base-writing port method
(`SyncWriteStore::{apply_sync_batch, reconcile_sync, apply_message_body}`,
`MessageCommandStore::destroy_message`):
- `BaseWrite::reconciler()` is `pub(crate)` to posthaste-domain-service — the
  sync sink, the reconcile pass, the settlement readback, and the lazy body
  persist mint it; **no other crate can** (teeth-checked: a foreign mint is
  `E0624` at compile time).
- `BaseWrite::legacy(reason)` is the loud escape hatch;
  `rg 'BaseWrite::legacy'` IS the violation inventory. Production grants: ONE
  (the draft-discard destroy, deleted with NS2). All others are test/bench
  base-seeding (a test legitimately plays the reconciler).
- The dead `MessageCommandStore::{set_keywords, replace_mailboxes}` port
  methods are DELETED (their `_tx` bodies survive as `#[cfg(test)]` seed
  helpers — follow-up: migrate those tests to sync-batch seeding and delete).
- The store's ~100 test call sites kept their signatures via `#[cfg(test)]`
  inherent shadows on `DatabaseStore` (inherent-over-trait precedence), so the
  seal cost zero test churn in the store crate.

The one-writer invariant is now a compile-time property. Next: **NS2** —
send/draft as single intents on this substrate
(RFC-L2-send-draft-state-machine M80–M83: the undo-send clock fix, intent-id
reconciliation, `moved_to_sent`'s deletion, the draft-discard grant's removal).
