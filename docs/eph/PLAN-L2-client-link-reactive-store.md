---
scope: L2
summary: "Implementation plan for the client-link reactive entity store — merged slices 2+3 of the reactive-store design: build the WASM entity store (generalized replica) and route mailbox counts + the mutation firehose through it, retiring the notification→REST-invalidation for store-owned entities. Aims at WASM (nightly-enabled, rolling to all)."
modified: 2026-06-26
reviewed: 2026-06-26
lifecycle: ephemeral
type: PLAN
depends:
  - path: docs/eph/DESIGN-L2-client-link-reactive-store
  - path: docs/replication/client-link/L2
  - path: docs/replication/client-link/L3
dependents: []
---

# Plan: client-link reactive entity store (merged slices 2+3)

Implements the merged second step of
[`DESIGN-L2-client-link-reactive-store`](DESIGN-L2-client-link-reactive-store.md):
build the **WASM entity store** (the replica, generalized from mail-list-scoped)
and route **mailbox counts** + the **mutation firehose** through it, retiring the
notification→REST-invalidation for store-owned entities. This is the slice that
makes the count/row divergence structurally unreachable.

## 1. Architecture (WASM-aimed)

The store lives in `posthaste-link-replica` (Rust), generalized from
`MailListReplica`, exposed via a new `posthaste-link-wasm` handle. It reuses
`posthaste-link-core`'s convergence engine unchanged (`predictor-single-crate`,
`convergence-core-shared-both-seams`). The TS adapter orchestrates transport +
IndexedDB outbox; the react-query cache is the subscriber view (a bridge writes
store notifications → cache).

- **Entities:** `message[id]` (summary projection + fold state), `mailbox[id]`
  (metadata + `unreadCount`/`totalCount` — **server-authoritative scalars**, not
  locally derived, because the store is partial), `view[viewId]` (ordered
  row-keys + coverage `(predicate, ranges)` from slice 1).
- **The firehose already exists.** `forward_notification`
  (`runtime/sessions.rs:645`) sends every scoped `DomainEvent` as a `Notification`
  frame on the session stream. So this plan does **not** add a firehose — it
  **replaces "invalidate-and-refetch" with "ingest-and-notify"** on a transport
  that's already running. That is the core simplification.
- **Mutations** are ingested; the store evaluates placement for evaluable
  predicates within `[TOP, W]` (place-or-ignore) and folds optimism via the
  existing per-message `MessageReplica`. **Paging** (scroll/jump/extend) is the
  only range-grower and still round-trips to the runtime.
- **Counts** ride the mutation stream as deltas, batched with the message event
  (atomic batch — your model), not a separate derivation.

## 2. Gaps to close (the real work)

| ID | Gap | Evidence |
| --- | --- | --- |
| G1 | Store core: register/subscribe, multi-entity, coverage, local eval, atomic-batch notify | `MailListReplica` is single-view, no subscribe, no counts (`link-replica/src/mail_list.rs`) |
| G2 | Firehose carries renderable+positional projection | `message.updated` payload has `keywords`/`mailboxIds` but **no `receivedAt`, no projection** (`store/mutations/projection_tracking.rs::message_updated_payload`) — a promoted never-held message can't be placed or rendered |
| G3 | Count deltas shipped on the stream, batched with the message event | `unread_emails`/`total_emails` are SQL-trigger-maintained (`store/db/schema/sql.rs:327`) but **no event announces count changes**; `mailbox.updated` carries only `{mailboxId}` |
| G4 | Gap-detection on the notification stream (design I3) | `spawn_notification_forwarder` silently drops on `Lagged` (`sessions.rs:625`); without resync the store silently misses mutations — the original bug, reincarnated |
| G5 | Retire `applyDomainEvent`/`invalidations.ts` for store-owned entities | `useDaemonEvents`→`applyDomainEvent`→`invalidateQueries({mailboxes, messagesRoot, …})` always-on (`domain-cache/invalidations.ts`) is the divergent second transport |

## 3. Staged sub-slices (each lands green, behind `VITE_RUNTIME_REPLICA`)

- **2a — WASM entity store core (Rust + tests).** Generalize `MailListReplica` →
  `EntityStore`: `message`/`mailbox`/`view` entities, register/subscribe by key,
  coverage `(predicate, ranges)`, local eval + place-or-ignore, atomic-batch
  notify (apply the batch, then notify once). Reuse one `MessageReplica` per
  message for the fold; counts are scalar fields on `mailbox[id]`. Pure compute,
  no transport. Tests: ingest a `message.created` batch → `mailbox[id].count`
  and `view[inbox]` both notified atomically; optimism fold/settle/revert;
  place-or-ignore by coverage.

  **LANDED (2026-06-26):** `posthaste-link-replica/src/entity_store.rs` —
  `EntityStore` with `message`/`mailbox`/`view` entities, `register_view` /
  `set_view_rows` / `ingest_batch` / `drain_dirty`, evaluable-predicate
  place-or-ignore against the `[TOP, W]` watermark (`SortKey` composite,
  `received_at`-based), atomic batch (one dirty drain per batch), count deltas as
  server-authoritative scalars, `Deferred` predicate left to the host. Exposed
  from `link-replica` lib. Coverage held as the single watermark (multi-range
  `CoverageRange` adopted with jump-to-date). 8 authoritative-placement tests;
  wasm32 + clippy clean.

  **Optimism LANDED (2026-06-26):** the store holds a `MessageReplica`
  (`Replica<MessageConvergence>`, the shared convergence engine) over message
  fold state. `accept_mutation` folds an assertion into the outbox;
  `message`/view placement read the *projected* state (base + pending), so
  optimism is a pure fold, never stored as truth. `settle(Confirmed)` retires the
  pending op (a visual no-op — the served base already carries the effect);
  `settle(Failed)` drops it and the projection reverts. Pending survives an
  unrelated base update (per-key `set_base` on ingest; the outbox is never
  cleared by a base update). A mutation on a not-yet-ingested message is tracked
  but deferred (folds in on ingest; its authoritative row is left untouched).
  The generic engine (`Replica<C: Convergence>`) makes optimism a property of
  the store, not message-specific — a future mailbox fold reuses the engine.
  Counts stay authority-only scalars (derived counts deferred to
  mutation-id-end-to-end). 8 optimism tests (flag-before-confirm, archive-drops,
  confirm-carries, failed-revert, pending-survives-rebase, destroy-revert,
  deferred-on-uningested); 26 link-replica tests green; wasm32 + clippy clean.
- **2b — Counts on the stream (runtime + store).** Emit `unreadCount`/`total`
  deltas batched with the message event (G3); store ingests into
  `mailbox[id].count`; sidebar reads from the store. **This kills the count/row
  divergence** (both are one store, one stream, one atomic batch).
- **2c — Firehose carries rows (runtime + store).** Enrich `message.updated`
  with `receivedAt` + the renderable projection (G2); the store materializes a
  promoted never-held message from the event; paging still fetches.
- **2d — Gap-detection (G4).** The notification stream detects a missed
  `sessionSeq`/`Lagged` and forces a resync (re-snapshot open views + refetch
  counts) — never a silent drop. Reuses the session collapse path.
- **2e — Retire invalidation (G5).** When the store is active, stop
  `applyDomainEvent` invalidating store-owned entities; remove
  `useDomainEventRefresh`/`eventMayAffectView` for those.
- **2f — Flip default.** Store on by default once real-browser-validated; the
  legacy REST/invalidation paths become dead code to remove (then realized
  `client-link/L2.md` §4/§5/§6 are rewritten).

### Host-contract preconditions (must land with/before 2e)

These came out of the optimism-fold quality review (`yoyvspnq`/`ukylrwov`).
The fold math and the divergence guarantees are sound; the risks are all in the
**host wiring contract** — the cutover (2e) is exactly where they fire, so they
gate 2e/2f.

- **P1 — Row implies a live base (HIGH).** `ViewRow` carries identity + position
  only; a row's *content* is read separately via `message(id)`, which returns the
  *optimistic* projection and `None` unless the message was separately ingested.
  `set_view_rows` places rows but does **not** seed message bases. So a snapshot
  that places rows for not-yet-ingested messages yields rows the host cannot
  render. The structural guard is good (a `ViewRow` cannot carry content, forcing
  the host through `message()`), but nothing guarantees row-placement and
  base-ingest arrive together. **Required:** a documented host-protocol invariant
  — every `set_view_rows` row id must have a live base, delivered atomically in
  the same batch — plus ideally a debug assertion. The current
  `accept_on_uningested_message` test encodes the blank-row state as acceptable;
  revisit it as a *deferred-mutation* case, not a *renderable-row* case.
- **P2 — Content-only mutations dirty `Message`, not `View` (MED).** A flag
  (membership unchanged) leaves the row in place, so `rederive_message` emits
  `DirtyKey::Message` but not `DirtyKey::View`. Correct and efficient, but a
  host that re-renders rows only on `DirtyKey::View` shows stale flag/read state
  forever — i.e. reintroduces the original bug class. **Required:** the host must
  maintain a message→rows reverse fan-out and re-render rows on
  `DirtyKey::Message`. Make it an explicit contract, not an assumption.
- **P3 — `ReplaceMailboxes` can transiently stomp concurrent authoritative
  state (MED, doc caveat).** It is absolute last-writer-wins, not a delta: an
  optimistic `ReplaceMailboxes([archive])` re-folded over an authority base
  that independently gained a label `[archive, important]` silently drops
  `important` until settlement. Not an idempotency violation (folding twice is
  stable) and bounded to the pending window, but the module doc's "retire is a
  visual no-op" overclaims — it is only true for the delta assertions
  (`SetKeywords`/`ApplyDiff`). Pre-existing in `MailListReplica`. **Required:** a
  doc caveat; consider whether mailbox moves should be carried as deltas
  (`ApplyDiff`) rather than absolute replaces where the client knows the
  before-state.

**Not a precondition (resolved in review):** an earlier "orphaned pending
resurrects stale optimism" concern was retracted — the outbox re-fold that
causes it is the *same* mechanism that correctly guards a keyword change from
being reverted by a sync (verified: optimism survives a re-served un-settled
base), and `message()`/`view_rows`/dirty-set stay consistent through a
delete+re-seed (no divergence). Whether a removal invalidates an in-flight
mutation is backend semantics (settled `Failed` → reverts correctly), not a store
bug; voiding pending on removal was rejected (it discards valid optimism in a
restore-from-trash). `MessageEntity.projection` was made private to close the
base-vs-optimistic leak (done).

## 4. Decisions (to confirm before 2b/2c)

- **D1 (settled):** WASM store in `posthaste-link-replica`, not a TS shell —
  unifies runtime + client on one convergence core. ✓
- **D2 (settled):** Enrich `message.updated` with the full `MessageSummary`
  projection. `MessageSummary` already carries `received_at` (sort position),
  `keywords` + `mailbox_ids` (membership), and the render fields — so there is no
  separate `receivedAt` to add; the current event payload is a diff, the fix is
  to attach the projection. Mutations are rare, so the wire cost is acceptable,
  and it avoids a promotion round-trip. **Lean confirmed: enrich.**
- **D3 (settled):** Attach `unreadCount`/`total` for the affected mailboxes to
  the message event, batched — atomic per batch (the design's `atomic-batch`
  MUST and the user's model), one derivation on the wire, one notify. Counts are
  trigger-maintained in the same write transaction, so reading the 1–2 affected
  mailboxes at event-emit time is a consistent point-read. **Attach site:**
  store-emit, in-tx (not at the runtime-forward boundary, or it re-introduces a
  read-after-commit race). Fallback if the sync path proves awkward: a separate
  `mailbox.updated{unreadCount}` event (convergent, loses per-frame atomicity).
  A `mailboxCounts` view is rejected — a second derivation, the most
  divergent-prone shape.
- **D4 (settled):** A `sessionSeq` cursor in the store/adapter (the client
  already tracks it in `sessionStorage`); on a gap or `Lagged`, trigger the
  existing session-collapse resync (re-snapshot all open views + refetch
  counts). Uniform across views + notifications. ✓

## 5. Verification

- **2a:** Rust unit tests over `EntityStore` — ingest a `message.created` batch
  asserts `mailbox[id].count` and `view[inbox]` are both notified in one batch;
  optimism fold/settle/revert; place-or-ignore by `[TOP, W]`; coverage tightening.
- **2b/2d:** a runtime harness test that on one `message.updated{created}` event
  the count delta and the row delta arrive on one stream and a simulated lag
  resyncs both (no stuck divergence) — the direct regression test for the
  reported symptom.
- **2e:** web tests that the sidebar unread count and the mail-list row update
  together from one ingested batch (parity with the current REST path), and that
  a dropped-and-resynced sequence still converges.

## 6. Relationship to the design

Implements `docs/eph/DESIGN-L2-client-link-reactive-store.md` slices 2+3 merged.
On completion (2f) it **supersedes** realized `docs/replication/client-link/L2.md`
§4 (per-event recompute), §5 (coverage), §6 (replicaAdapter), which are then
rewritten. It **preserves** `predictor-single-crate` and
`replica-rebase-only`; it **refines** `one-replica-both-seams` per the design's
seam split (convergence core shared; windowing/coverage client-only).
