---
scope: L2
summary: "Redesign the client↔runtime data flow as one reactive entity store (the WASM replica, generalized) fed by a single ordered mutation stream, with coverage expressed as sort-key intervals per predicate — removing the parallel count/row derivations and hand-maintained affects predicates that let a new message move the unread counter without appearing as a row."
modified: 2026-06-26
reviewed: 2026-06-26
lifecycle: ephemeral
type: DESIGN
depends:
  - path: docs/replication/client-link/L1
  - path: docs/replication/client-link/L2
  - path: docs/eph/DESIGN-L2-optimistic-projection
  - path: docs/state/mail/L2
dependents: []
---

# Client-link reactive store: one derivation, interval coverage

## 1. The problem

Live updates sometimes drop until reload: a new message arrives and the unread
counter moves, but the row does not appear (or does not appear at all) until the
view is refreshed. This is unreproducible case-by-case because it is not one
logic bug — it is **two derivations of the same fact drifting apart** under
timing and burst conditions:

- **Rows** come from the runtime `mailList` view, recomputed per domain event,
  narrowly gated by a hand-written `event_affects_view` predicate, equality-
  suppressed, and delivered over three independently-bounded broadcast hops
  each with its own lag recovery.
- **Counts / nav / badges** come from a *separate* `useDaemonEvents` path that
  broadly invalidates react-query keys and REST-refetches on every notification
  — always on, regardless of the view-frame flag.

"Is there new mail in the inbox?" is therefore answered **twice, on two
transports, with two relevance predicates** that must agree. The count path is
broad enough to self-heal on any later event; the row path is precise enough to
miss. Hence the counter wins and the row loses.

### Invariants the current flow violates

- **I1 — Single derivation.** Each UI-visible fact is produced by one projection.
  Rows and counts are two projections of the same canonical state today.
- **I2 — No hand-maintained relevance predicates.** "Does this affect the inbox?"
  is authored in three places (`event_affects_view` Rust, `eventMayAffectView`
  TS, `invalidations.ts` routing) that must agree.
- **I3 — Ordered, gap-detecting delivery.** The notification forwarder silently
  drops on `Lagged`; the view pump re-snapshots on `Lagged`. Same bus, asymmetric
  recovery → divergence under burst.
- **I4 — Convergence.** Counts self-heal on any event; rows depend on the
  precise gated path. They converge at different rates and can sit divergent
  indefinitely.

You cannot harden the row path without moving the race. You fix it by deleting
the second derivation.

## 2. The target: one reactive entity store

The client holds a **generalized replica** — a normalized, keyed entity store
with a register/subscribe interface — generalizing today's mail-list-scoped
`posthaste-link-replica` (`MailListReplica`) into the "coverage/atoms layer"
that crate's own doc defers to. It is fed by a **single ordered stream** of
mutations from the runtime; it is the only thing the renderer subscribes to.

- **Entity kinds:** `message[id]`, `mailbox[id].count` (scalar), `view[viewId]`
  (ordered row-keys + coverage), `smartMailbox[id]`, `conversation[id]`,
  `account[id]`, plus app state. Each is a keyed register.
- **Subscribers** declare a key dependency (`mailbox[inbox].count`,
  `view[inbox]`, `message[m]`) and are notified only when that key changes —
  fine-grained, not broad invalidation.
- **Lists are server-maintained view entities, scalars are client keys.** A list
  (`view[viewId]`) holds an ordered set of row-keys plus coverage; its row data
  is read from `message[id]` entities the store also holds. The inbox *list* and
  the inbox *unread count* are different entity kinds, but both live in one
  store fed by one stream — so they move together.
- **Batches are the commit boundary.** A `message.created` is shipped with its
  `mailbox.count.updated` in one atomic batch; the store applies the whole batch,
  then notifies subscribers once. A count and a row notified separately inside
  the client is the divergence, reincarnated one layer down.

### Shared across both seams, or client-only

The store bundles two capabilities, and only one belongs on both seams:

- **The convergence core** — `posthaste-link-core`'s predictor + outbox fold +
  settle. **Shared across both seams** (`one-replica-both-seams` /
  `predictor-single-crate`, preserved): the client folds its outbox over
  runtime-served state; the runtime folds its outbox over backend (provider)
  state. Neither reimplements the fold.
- **Windowing + interval coverage + register/subscribe** — **client-only.** This
  machinery exists to manage *partiality*: the client holds a windowed subset of
  a query and must tell "absent because unchanged" from "absent because not
  held." The runtime is not partial — it is the full-mirror authority (the
  canonical SQLite store syncs the whole account's metadata; bodies are a
  separate lazy resource), so it has no view-windows over its own corpus. Its
  coverage w.r.t. the backend is **sync-cursor-based** — per-mailbox
  `highest_uid` / `highest_modseq` high-water-marks (`imap_mailbox_sync_state`),
  a one-dimensional cursor, not sort-key intervals. Putting interval coverage
  runtime-side would duplicate the authority's own bookkeeping and reintroduce a
  second derivation.

The runtime-side piece that *is* new is a **per-client coverage ledger**: the
authority tracks each client's `(predicate, ranges)` to scope deferred-predicate
deltas and decide what to serve on paging. That is authority bookkeeping *about*
client state — same coverage tuple shape, different role — not a windowed replica
of the backend.

## 3. Coverage: sort-key intervals per predicate

`RuntimeCoverageKind { Complete, Partial, Unknown }` is deleted. It is hardcoded
`Complete` for windowed lists (`views.rs::complete_coverage`), so a 50-row
window over a 5000-message inbox is stamped `Complete` — false, not merely
unexpressive. The real coverage already exists as `MailListContinuation` cursors
and is promoted into the general model:

> **Coverage is a set of sort-key ranges over the query's total order, paired
> with the predicate.** `coverage(view) = { order, predicate, ranges }` where
> each range is `[from, to]` in the composite sort-key domain
> (`(sortField, dir, id)`).

A query defines a total order; coverage is the set of ranges within which the
client holds **every matching row, with no gaps**. The invariant:

> `range [TOP, W]` ≡ every matching row with key ≤ W is held ≡ every *unheld*
> matching row has key > W.

- **Typical window:** one range `[TOP, W]`, `W` = the last held row's key.
- **Jump-to-date:** a range `[a, b]` not anchored at TOP — the gap above is
  first-class "unknown," not an untracked hole.
- **Scroll + jump + scroll:** multiple disjoint ranges with gaps between them.
- **"Complete"** is the degenerate observation that a range reaches BOTTOM (and
  TOP), not a tracked state. Partial-with-a-watermark is the normal case.

Sort-key intervals (not offsets/counts) because offsets shift on every
insert/delete while a sort-key boundary is durable.

## 4. How state changes: mutations absorb, paging grows

Two operations, exactly:

- **Mutations (the firehose).** Every message mutation is delivered to the
  client. For an **evaluable** predicate (structured: `in:inbox`, `is:unread`,
  where the client holds the filterable atoms), the client runs **one local
  evaluation**: does the mutated message match the predicate *and* is its sort
  key in `[TOP, W]`? If yes → place it (insert / move / update); if no → ignore.
  "In coverage vs out of coverage" is the *result* of this evaluation, not a
  precondition branched on.
- **Paging (scroll / jump / extend).** The *only* mutation-independent
  round-trip: the runtime returns the next page and the coverage range grows
  downward toward BOTTOM.

> A mutation can **shrink or hold** the covered range, never grow it downward.
> Only paging moves `W` toward BOTTOM. New arrivals land at TOP (inside the
  range) and are absorbed; if the window is size-capped the tail evicts and `W`
  moves *up*. Nothing a mutation does extends coverage past `W`, because past
  `W` is precisely what the client does not hold.

### Watermarks are locally maintainable

When the watermark row itself mutates and moves (e.g. the 50th message is read
under `unread-first` sort), tightening the watermark to the new last held row is
**always a valid tightening** — every unheld matching row still has key `> W`,
so the invariant holds automatically for any `W' ≤ W` drawn from held rows. The
arithmetic never needs the unheld corpus. The residual constraint is not
watermark math; it is delivery scope (§5).

### Deferred predicates

For a **deferred** predicate (full-text search, complex smart-mailbox rules the
client cannot evaluate), the client self-maintains nothing. The runtime
recomputes membership against the full corpus and authors the in-window delta
(`remove X`, `insert Y@pos`, new `order`) plus the updated `W`, which the client
applies verbatim. The runtime holds the open view and its window, so it can
scope the delta to the client's coverage.

## 5. The decision matrix

| | **Evaluable predicate** | **Deferred predicate** |
| --- | --- | --- |
| **Mutation** | Local: evaluate against `[TOP, W]`, place-or-ignore. Never round-trips. | Defer: runtime authors the in-window delta. |
| **Paging** | Defer: runtime evaluates the page. | Defer: same. |

The runtime owns out-of-coverage and deferred predicates; the client owns
evaluable in-coverage. Nothing is maintained twice, and the runtime stops
recomputing evaluable views per-event entirely — the per-event recompute and
the `event_affects_view` predicate disappear for the common case. Heavy
evaluation survives only where unavoidable: paging and deferred predicates.

## 6. Load-bearing invariants

These hold the model together; without any of them it silently corrupts into
the original bug:

1. **The firehose carries render-and-position projection, not field diffs.** A
   mutation can promote a never-held message into the window (an unread-toggle
   on a below-`W` message). To *decide* it enters (sort key computable from
   `received_at` + keyword) *and* to *render* it, the event must carry enough of
   the row to materialize it. Mutations are rare; this is cheap.
2. **The stream is gap-detected.** This is the *only* remaining way the model
   silently corrupts: drop one mutation and a reorder-in is lost — the original
   bug, one layer down. On lag/reconnect the stream forces a resync (re-snapshot
   open views + re-establish the firehose cursor), never a silent skip. The
   local-eval fast path is sound only on a gap-detected channel.
3. **Deferred predicates author their own deltas.** A view whose predicate the
   client cannot evaluate must receive an authority-authored membership delta;
   it never self-evaluates and never guesses placement.

### Coverage invariants (from the watermark pressure test)

4. **Coverage is per-predicate, not per-position.** "I cover `in:inbox` over
   `[TOP, W]`" does not imply "I cover all messages over `[TOP, W]`." Messages
   below `W` that were never matching are not held.
5. **Discovery is authority-driven.** Evaluability governs *placement/removal
   of told-about mutations*, never finding messages the client was never sent.
6. **Eviction breaks window-fullness, not coverage.** "Complete-for-range but
   short" is a first-class valid state. Backfill-to-full-window is a deferred
   page fetch, never a local guess.
7. **Optimism never authors coverage.** The outbox overlays rows on top of the
   authority base; it never moves a watermark. Coverage is authority-
   single-writer; settle/reject reasserts the base and the overlay reconciles.

## 7. Optimism's place

Local mutations overlay on the authoritative base via the existing
`posthaste-link-core` predictor (`accept` / `apply_base_update` / `settle` /
`project`) — the same convergence engine the mail-list replica uses today,
generalized to all entity kinds. The outbox is a separate overlay; it never
authors coverage. A rejected mutation settles to `Failed`; the authority's base
correction reasserts truth and the overlay reconciles automatically. This
preserves the double-settlement lifecycle already built (optimistic → runtime
confirm/correct → re-serve → re-fold).

## 8. What current code gives way

| Current | Disposition |
| --- | --- |
| `RuntimeCoverageKind { Complete, Partial, Unknown }` (`runtime-contract`), hardcoded `Complete` for windowed lists | **Removed.** Replaced by `(predicate, ranges)`. |
| `MailListContinuation` completeness (`has_before`/`has_after`) | **Moved** into coverage range bounds — `has_after=false` becomes `to: None` (reaches BOTTOM); the boolean completeness is now derived from the range. |
| `MailListContinuation` opaque fetch cursors (`before_cursor`/`after_cursor`) | **Retained** for paging — they are fetch tokens, not sort-key boundaries, and are distinct from coverage. (Slice 1 leaves them in place; the client still reads them.) |
| Per-event `event_affects_view` / `message_event_affects_list` (`runtime/views.rs`) + `recompute_view_if_changed` for evaluable views | **Removed for the common case.** The runtime forwards mutations; evaluable views are client-maintained. Recompute survives only for deferred views and paging. |
| `useDaemonEvents` → `applyDomainEvent` / `dispatchDomainEvent` → `invalidations.ts` REST-refetch (the count/nav/badge path) | **Removed.** Counts and nav become store entities over the one stream. |
| `useDomainEventRefresh` + client `eventMayAffectView` | **Removed.** No client reaction to raw events; subscribers read the store. |
| `MailListReplica` (mail-list-scoped) | **Generalized** to the reactive entity store (`posthaste-link-replica`), reusing `posthaste-link-core` unchanged. |
| Per-view broadcast + per-session broadcast (3 hops, asymmetric lag handling) | **Collapsed** to one ordered per-session frame/mutation log with uniform gap-detected resync. |

## 9. Relationship to the realized client-link L2

This is a forward design; `docs/replication/client-link/L2.md` remains accurate
for the **current** mail-list-scoped implementation. On realization this design
**supersedes**:

- L2 §4 (delta computation / per-event recompute) — the per-event re-query is
  removed for evaluable views;
- L2 §5 (working-set coverage) — the coverage model is replaced by intervals;
- L2 §6 (replicaAdapter) — the adapter backs the generalized store.

It **preserves** L2 §1 (`posthaste-link-core`, single predictor), §3 (the
JSON-string WASM boundary), and the `replica-rebase-only` invariant.
`one-replica-both-seams` is **refined**: the convergence core (predictor + outbox
fold) stays shared across both seams, but the windowing / coverage /
register-subscribe machinery is client-only — the runtime is the full-mirror
authority and does not window its own corpus (see §2). The realized L2 should be
rewritten when the first slice lands; until then it stays as the description of
shipped code.

## 10. Rollout (indicative)

1. **Coverage as intervals** — replace `RuntimeCoverageKind` with
   `(predicate, ranges)`; promote `MailListContinuation` into ranges. Pure
   representation change; behavior-preserving. **LANDED** (`RuntimeCoverage` is now
   `{ ranges: Vec<CoverageRange> }` with `CoverageRange { from, to }` over the
   composite sort-key domain; `mail_list_state` derives an honest `[TOP, W]`
   window from the served page + `has_after` instead of stamping `Complete`; the
   opaque fetch cursors stay on `continuation` for paging. `openapi.json`, both
   `schema.gen.ts`, and `apps/web/src/runtime/types.ts` regenerated; the web
   client never read `.coverage`, so behavior is unchanged.)
2. **Counts as store entities over the stream** — ship mailbox/nav counts on
   the frame stream; delete their notification-invalidation. Vertical proof of
   the model; kills the count/row divergence; reversible behind the existing
   `useRuntimeViewFrames` flag.
3. **Generalize the replica** — `MailListReplica` → entity store; views
   register as `view[viewId]` entities; subscribers replace react-query
   invalidation.
4. **Collapse the channel** — one ordered per-session log; uniform gap-detected
   resync; remove `useDaemonEvents`/`useDomainEventRefresh`/`eventMayAffectView`.
5. **Delete the per-event view recompute** for evaluable views once the store
   self-maintains.

## Assertions

| ID | Sev. | Assertion |
| --- | --- | --- |
| single-derivation | MUST | Each UI-visible fact (a row, an unread count, a nav badge) is produced by one projection of the store; no parallel notification→REST-refetch path exists. |
| one-ordered-stream | MUST | All client state arrives via one ordered mutation/frame stream; there is no second event-reaction transport. |
| gap-detected-delivery | MUST | On lag or reconnect the stream resyncs (re-snapshot open views + re-establish the firehose cursor); it never silently drops an event. |
| firehose-carries-rows | MUST | Mutation events carry renderable and positional projection (enough to evaluate membership and render the row), not field-only diffs. |
| coverage-as-intervals | MUST | View coverage is `(predicate, sort-key ranges)`; the `RuntimeCoverageKind` enum is removed. |
| coverage-authority-authored | MUST | Coverage ranges are written by the authority on paging; the client records them and never invents or extends a range downward on its own. |
| mutations-absorb-or-ignore | MUST | For an evaluable predicate, a mutation is one local evaluation: place if the predicate matches and the sort key is in `[TOP, W]`, else ignore; it never grows the range. |
| paging-grows-range | MUST | The covered range grows only via a runtime page fetch; a mutation may shrink or hold the range, never extend it past `W`. |
| deferred-predicates-author-deltas | MUST | A view whose predicate the client cannot evaluate receives authority-authored membership deltas; it never self-evaluates placement. |
| optimism-never-authors-coverage | MUST | The outbox overlays rows on the authority base; it never moves a coverage watermark. Coverage is authority-single-writer. |
| atomic-batch | MUST | A batch applies to the store atomically and notifies subscribers once after commit; a count and its row never notify as separate client-side events. |
| predictor-single-crate | MUST | The store reuses `posthaste-link-core`'s convergence engine (`accept`/`apply_base_update`/`settle`/`project`); no second predictor is introduced. |
| convergence-core-shared-both-seams | MUST | The predictor + outbox fold + settle machinery is the same on the client↔runtime seam and the runtime↔backend seam; neither reimplements the fold. |
| coverage-lives-on-partial-nodes | MUST | Interval coverage and view-windowing exist only on partial nodes (the client); the runtime, as the full-mirror authority, does not window its own corpus — its backend coverage is sync-cursor-based, not sort-key intervals. |
