---
scope: L1
type: DESIGN
lifecycle: ephemeral
summary: "Client-side optimistic replication. Server emits object-scoped post-state assertions over a typed event contract; client applies them idempotently into a local replica that drives all views. Two-surface API end-state (commands + sync stream); HTTP list/detail endpoints retire. Seq for ordering, per-(account,type) tokens for recovery scope, authoritative resnapshot on gap-too-old."
modified: 2026-06-02
reviewed: 2026-06-02
status_note: "Approved for implementation 2026-06-02. Phase B (backend event-contract hardening) starts first; the rest follows in sequence."
depends:
  - path: docs/L1-sync
  - path: docs/L1-api
  - path: docs/L1-ui
  - path: docs/L1-jmap
  - path: docs/L0-sync
  - path: docs/L0-jmap
  - path: docs/eph/DESIGN-L1-client-read-models
dependents: []
---

# DESIGN: Optimistic replication & client reconciliation

## Status

Approved 2026-06-02. Implementation starts with phase **B** (backend event-contract hardening); the frontend phases follow once B's typed envelope is in place. This supersedes the ad-hoc per-event invalidation approach in `domainCache.ts`.

## TL;DR

The client is a server-authoritative **replica**. Events are **idempotent post-state assertions** of per-object state — applied through one primitive on the client. Optimistic mutations are predicted assertions that the authoritative diff later confirms (no-op) or corrects. **The server is the authority for per-object state; the client is the authority for per-view state** (filters/sorts/window/derived rendering). Views are derived locally from the replica, so per-object updates don't trigger view recomputation — eliminating the blink, the echo-suppression hack, and the rotting per-event invalidate/skip matrix.

The end-state API surface is minimal: **commands** (HTTP write), a **snapshot read** (scope-parameterized, used for bootstrap and resnapshot), and the **event stream** (cursor-parameterized; backlog + live tail in one connection — catch-up and live-following are different cursor values, not different modes). The snapshot read and event stream return the **same assertion envelope** to the same client apply path. HTTP list/detail endpoints retire — the replica answers reads locally. Attachment HTTP stays as plain file transfer.

The backend is already a server-authoritative replica against providers (state tokens, `*/changes` deltas, `cannotCalculateChanges` → authoritative resnapshot, monotonic `seq` event log). We extend that same discipline across the last hop into the browser, and harden the emission contract so it holds uniformly across object types (phase B) rather than by per-call-site convention.

## Problem

The client cache reconciles against live `/events` via a per-topic handler map (`apps/web/src/domainCache.ts:504-625`, exhaustive `satisfies Record<DomainEventTopic, EventHandler>`). That map has bifurcated into two incompatible disciplines, and the split is the source of a visible UI defect.

**The blink.** A mailbox move (e.g. Archive) is applied optimistically by filtering the row out of the source list via `patchMessageListMembership` (`mailState.ts:683-690`). On command success the runner fires `invalidateMessageMutationReadModels`, invalidating `queryKeys.messagesRoot` — every message-list infinite query (`domainCache.ts:237-250`) — fire-and-forget (`OperationsProvider.tsx:209`). That triggers an immediate refetch of the active list (`MessageList.tsx:219-234`). But the list read model is eventually consistent: the move can return success before the message is re-indexed out of the source mailbox projection, so the refetch returns a page that *still contains the just-removed row*, overwriting the optimistic filter. The row reappears, then a later refetch removes it again — the blink.

**The smoking gun: an inconsistent patch/invalidate matrix.** The backend emits concrete diffs for both keyword and mailbox changes — `append_message_diff_events_tx` snapshots the pre-image via `fetch_message_before_apply_tx` and emits `keywords` on `message.keywords_changed` (`mutations.rs:577-580`) and `mailboxIds` on `message.mailboxes_changed` (`mutations.rs:593-596`). The frontend treats them asymmetrically:

| Topic | Backend payload | Frontend handler |
|---|---|---|
| `message.keywords_changed` | `messageId`, `keywords[]` (`mutations.rs:577-580`) | **Applies the diff.** `applyKeywordEventPatch` re-applies the keyword diff in place into detail/view/summary/list rows (`domainCache.ts:593-606`). |
| `message.mailboxes_changed` | `messageId`, `mailboxIds[]` (`mutations.rs:593-596`) | **Invalidate-only.** Never reads `payload.mailboxIds` (`domainCache.ts:610-614`). |
| `message.arrived` | `messageId`, `mailboxId` (`mutations.rs:607-610`) | Invalidate-only (`domainCache.ts:589-592`). |
| `message.updated` | `messageId`, `conversationId`, `created` (`mutations.rs:562-567`) | Invalidate-only (`domainCache.ts:615-618`). |

A mailbox-membership diff-patch is available on the wire but discarded — even though `applyMailboxPatch` already exists (`mailState.ts:711-778`) and is wired only into local optimistic mutations. Keyword changes survive an SSE round-trip because both the optimistic patch and the live event mutate rows in place; mailbox moves bottom out in an eager list invalidation against an eventually-consistent projection. That asymmetry *is* the blink.

This matrix rots. Every new optimistic surface (move, archive, tag, compose-sent reflection, account ops) forces a per-topic decision: patch in place, or invalidate? With no governing model the default is invalidate — which reverts optimism and forces the echo-suppression hack to exist.

**The echo-suppression hack it forces.** The client maintains `localMutationEvents` keyed by `(accountId, messageId, topic)` with a 5s TTL (`mailState.ts:125-126,176-180`), consumed single-use in `shouldSuppressLocalEcho` (`mailState.ts:214-229`) before `applyDomainEvent` (`useDaemonEvents.ts:115`). The key omits `seq` and payload, so a local mutation racing an unrelated remote change of the same `(message, topic)` within 5s can silently swallow the remote event. This hack only exists because diff application is not idempotent: re-applying the same assertion *should* be a no-op, and if it were, there would be nothing to suppress.

## What we already have

The principled model already runs — one layer down, between the engine and providers. The discipline just stops before the client.

- **Per-type, per-account state tokens.** `sync_cursor(account_id, object_type, state, ...)` (`crates/posthaste-store/src/sync_state.rs:7,35`), advanced atomically inside the same SQLite transaction as the data they describe (`mutations.rs:281-283`), assembled from `mailbox_sync.cursor` / `email_sync.cursor` (`crates/posthaste-engine/src/live_sync.rs:79-89`). RFC 8620 per-type state, documented in [[L1-sync]] (`docs/L1-sync.md:190`).
- **`*/changes`-style delta diffs.** `fetch_email_delta` loops `Email/changes`, maps `destroyed()` → deletes, `created()`+`updated()` → `Email/get` (`crates/posthaste-engine/src/sync.rs:172-237`); `fetch_mailbox_delta` likewise. Disjoint created/updated/destroyed — the JMAP delta shape.
- **`cannotCalculateChanges` → authoritative full snapshot prune.** Delta layer catches `GatewayError::CannotCalculateChanges` (`live.rs:355`) and converts to a full snapshot (`sync.rs:54-60,79-85`). `apply_sync_batch_tx` set-differences local IDs against the snapshot and deletes stale rows before persisting the new cursor (`mutations.rs:58-130`).
- **A monotonic, durable, seq-ordered event log.** `event_log.seq INTEGER PRIMARY KEY AUTOINCREMENT` (`db.rs:151-159`); every event is created by `insert_event_tx`, which stamps `seq = tx.last_insert_rowid()` inside the mutation txn (`projections.rs:347-379`). `/events` replays the backlog from `afterSeq` and filters live to `seq > replayed_through` for gap-free ordered delivery (`api.rs:2766-2811`). **The event_log is not pruned** (the only `DELETE FROM event_log` is on account removal, `source.rs:101`), so the log can serve any cursor in retention.

The backend is already a server-authoritative replica with state tokens, ordered diffs, idempotent-by-construction snapshot reconciliation, and a monotonic cursor. The client consumes the same seq-numbered stream — and throws the diffs away.

## Backend event contract: principled for messages, not yet enforced

A fair worry before building a client replica on this stream: does the backend support replication *as a contract*, or does `message.mailboxes_changed` carry `mailboxIds` by luck? Auditing the emission layer, the answer is *in between* — and it must be hardened (phase B) before the replica generalizes past messages.

**Two emission paths, both hand-authored.**

- **Sync ingestion** is principled: `apply_message_record_tx` snapshots a pre-image (`fetch_message_before_apply_tx`, `mutations.rs:322,335-368`) and `append_message_diff_events_tx` derives events from the before/after delta (`mutations.rs:548-610`). A real diff function — the pattern we want.
- **Command handlers** (user actions) hand-emit per handler: `set_keywords_tx` emits `keywords` (`mutations.rs:919-926`), `replace_mailboxes_tx` emits `mailboxIds` + `arrived` (`mutations.rs:974-994`), `destroy_message_tx` emits `{deleted:true}` (`mutations.rs:1031-1038`). Correct *today* only because each author wrote the right payload — per-call-site discipline, not a guarantee.

**No structural enforcement.** Every payload is a free-form `serde_json::json!({...})` literal, read by the client as an untyped `Record<string, unknown>`. Nothing at the type level guarantees that a mutation emits an event, that the event carries the *complete* post-state of every field it changed, or that field names match what the client reads. A renamed field or a forgotten emit is a silent, runtime-only break.

**Duplicated, and message-only.** Sync and command paths each re-implement "a change becomes these events." And the diff discipline is essentially message-scoped: `mailbox.updated`, `settings.updated`, `account.*`, `sync.*` are coarse "something changed" signals (`mutations.rs:84-90,140-146,201-206`) the client can only answer with invalidation.

## The model

Treat the client cache as a **server-authoritative replica**. Two distinct authorities:

- **Per-object state** (a message's `mailboxIds`, `keywords`, `subject`, …) is the **server's** authority. It travels to the client over the event channel as **idempotent post-state assertions**: "object X's current state is S." Applying the same assertion twice equals applying it once — assertions are *not* operational deltas. This is the key property that makes everything else work.
- **Per-view state** (filters, sorts, the loaded window over a list, the rendered DOM) is the **client's** authority. Views are derived locally from the replica: `view = replica.derive(filter, sort, window)`. The server never tells the client "what's in this view" except at boundaries (initial fetch, scroll-to-next-page, server-only filter queries).

**Optimistic mutations are predicted assertions.** A local move predicts "X moves to Archive" and patches the row. The authoritative event later asserts the same post-state — which, applied idempotently, is a no-op (it *confirms* the prediction). If the server disagrees (the prediction was wrong, or a concurrent change won), the authoritative assertion replaces the row's state and *corrects* the view. Either way, no list refetch.

**The replica is the uniform abstraction.** Every view in the UI is `replica.derive(predicate)`. The view layer doesn't know how the replica is kept current — that's a strict implementation detail behind the abstraction.

This single property dissolves both current hacks:

- **The blink dies.** The optimistic filter removes X from the source list; the live event asserting X's new `mailboxIds` re-derives the same membership — X stays out. No list invalidation, so no refetch races a lagging projection.
- **Echo suppression dies.** Re-applying the local prediction's own echo is a no-op by construction; the suppression hack and its event-swallowing race delete with it.

This is the JMAP shape the project already speaks: per-type state + `*/changes` deltas + idempotent apply is the email-native instance of server-authoritative optimistic replication ([[L0-sync]] `docs/L0-sync.md:23`; [[L1-jmap]] `docs/L1-jmap.md:43-47`). We extend the engine's discipline across the last hop into the browser.

## The event-diff contract (post phase B)

For idempotent application to hold uniformly, the event contract must be **tight, typed, and post-state**. After phase B:

- **Object-scoped update events, not field-scoped.** A single `message.updated` event carries the full post-state of the changed message: `{messageId, mailboxIds, keywords, conversationId, …}`. Field-scoped topics (`keywords_changed`, `mailboxes_changed`) collapse into the object-scoped `updated`. Trivially idempotent (apply post-state, idempotent by definition), trivially collapsible (one event per touched object in catch-up).
- **JMAP-shaped envelope:** `{type, kind: 'created' | 'updated' | 'destroyed', id, account, fields}`. Destroys carry no `fields` — they're terminal assertions. Same envelope across object types; the client reconciler has one shape for all of them. Coarse types where row-diff is meaningless (config/account-runtime) are *explicitly* documented as coarse, not accidentally so.
- **Typed payload structs, generated both sides.** Rust serde structs per topic in the store/domain crates; `asyncapi.json` is the generated source of truth for client event types, mirroring `openapi.json` for REST. The compiler then guarantees a `MessageUpdated` carries `mailboxIds`; a field-rename is a build break, not a silent runtime drift.
- **Single mutation→event chokepoint.** The command path is routed through the same pre-image/diff helper the sync path already uses (`append_message_diff_events_tx`, generalized). "The row changed" produces events in exactly one place — removing the duplication and the per-handler payload authoring.
- **Convergence property test.** A test that replays a mutation's emitted events onto a fresh client replica reproduces the server's post-state. This is the structural enforcement: any mutation that emits an incomplete diff fails the build.

## Per-view derivation

Views are computed from the local replica. The same primitive handles four cases:

- A locally-initiated optimistic mutation (your own click) — patches the row, runs the view-membership logic.
- A live event from another client — same.
- A catch-up assertion after reconnect — same.
- The dedupe-on-merge when a next-page fetch returns rows already in the replica — same.

### Membership + position

When a row's post-state arrives (or is locally patched), per affected view:

1. **Filter membership.** Does the row belong in this view at all? Mailbox membership is `mailboxIds.includes(view.mailboxId)`; tag/read filters likewise — all evaluable from the row's post-state.
   - If membership changed to `false` and the row is loaded → remove.
   - If membership changed to `true` and the row is loaded → update in place.
   - If membership is `true` and the row is *not* loaded → insertion case (next step).
   - For server-only views (full-text relevance, opaque smart-mailbox rules): the client can't evaluate locally → fall back to a **targeted single-list refetch** of that one view. Narrow, not "invalidate the world."

2. **Position within the loaded window.** Given the view's sort key (e.g. `receivedAt DESC`):
   - `new.sortKey > top.sortKey` → **prepend.**
   - `top.sortKey ≥ new.sortKey ≥ bottom.sortKey` → **insert at compared index** within the loaded window.
   - `new.sortKey < bottom.sortKey` → **no-op.** The row belongs in unloaded territory beyond the cursor; the next-page fetch will return it when the user scrolls.

### Pagination and the cursor as window boundary

Each loaded page carries a `nextCursor`. The cursor is the server-side boundary between "loaded" and "the server will give it to me when I ask." Local insertion *never moves the cursor* — the next-page fetch still resumes from the same anchor.

On next-page merge, **dedupe by id** while appending: a row the client locally inserted may also appear in the server's next page; dedupe handles it idempotently. This is the same idempotent-merge property as elsewhere.

### Sort stability

The sort key tiebreaker (typically `(receivedAt DESC, id DESC)`) must match the server's pagination cursor's ordering. Mismatched orderings produce off-by-one weirdness at page boundaries; matched orderings make local position math agree with what the server would have placed.

### Sort keys travel in the post-state

Object-scoped update events carry the fields needed to compute local position: `receivedAt` for date sort, `subject` for subject sort, etc. The client never needs to interpret an opaque sort — for client-evaluable orders the comparison is a one-liner; for server-only orders the row carries the score/rank as another field.

## API surface (current → target)

**Today** (three+ surfaces, conflated responsibilities):

- HTTP read endpoints — message lists, conversation views, message details, tags, mailboxes, smart-mailbox queries. Each combines "which rows belong here" with "their per-object state."
- HTTP command endpoints — per-command, `POST /sources/{}/commands/messages/{}/…`.
- SSE stream — `/events`, separate envelope, separate semantics.
- Bootstrap — `POST /read`.
- Asset transfer — blob/attachment HTTP.

**Target** (two assertion-producing operations + commands + assets):

There are **two genuinely distinct server operations** that deliver post-state assertions, differing only in their input shape; both feed the same client apply path:

- **Snapshot read** (`POST /read`, scope-parameterized) — "give me the current state of objects matching this scope." Returns a finite stream of current-state assertions plus a cursor at the end so the client knows where to resume the live stream. Used for **bootstrap** (initial population, scope = what the user wants loaded) and **authoritative resnapshot** (recovery from too-old gap, scope = the diverged `(account, type)`). Generalizes the existing `POST /read` endpoint to return the same envelope events use.
- **Event stream** (`GET /events?afterSeq=N[&collapse=byId][&scope=...]`, cursor-parameterized) — "give me everything since seq N, then keep streaming live." Backlog from N (collapsed by default — idempotent assertions make intermediate states redundant) transitions seamlessly to the live tail in the same connection. Used for **catch-up** (cursor = last applied seq) and **live-following** (cursor = current head); these are not distinct modes, just different cursor values for the same operation.

Plus:

- **Commands** — single HTTP envelope, accepts mutation batches, responds with the resulting post-state assertions in the same envelope. All writes.
- **Assets** — plain HTTP for blob/attachment file transfer. Different concern; stays as-is.

The unification is at the **output layer** (one envelope, one client apply path); at the input layer snapshot and stream are honestly different — a snapshot is a query against current state (cheap, indexed), a stream is a walk of history (replay). Forcing them into one endpoint would conflate those, and a year-old client would replay millions of log entries instead of issuing one snapshot read. Two operations is the honest minimum.

HTTP list/detail endpoints retire. The replica answers reads locally; client view-derivation handles filtering/sorting/windowing. The server stays stateless about per-client views.

## Catch-up & desync recovery

A monotonic `seq` makes divergence detectable, which today it is not. The client stores `payload.seq` blindly with no `+1` contiguity check (`useDaemonEvents.ts:110-113`), and backend broadcast `Lagged(n)` is swallowed by the live stream's catch-all `_ => None` (`api.rs:2803`). The recovery ladder:

1. **Steady state — apply.** Each live event contiguous in `seq` is applied through the one idempotent primitive. No invalidation.
2. **Gap detected (`seq > last + 1`) — collapsed catch-up.** Pull `/events?afterSeq=last&collapse=byId`. The server walks the log, returns at most one assertion per `(account, type, id)` since `last` (destroys take precedence over earlier mutations for the same id), in seq order, same envelope. Client applies each through the same primitive. Bounded by changed-objects, not events. This works because of post-state assertion semantics: applying N assertions for the same object equals applying the latest one.
3. **Cursor too old or scope-divergence — authoritative resnapshot.** If the backlog no longer reaches `last`, or a per-(account,type) state token has drifted, perform an authoritative read via `POST /read` for that scope and **prune local rows absent from the snapshot** — the client analog of `replace_all_messages` (`mutations.rs:95-130`), and the RFC 8620 `cannotCalculateChanges` contract the backend already implements with providers (`docs/L1-sync.md:197`).

**Invalidation's correct, narrow role is step 3's trigger** — not a per-event default.

### Cursor model: hybrid, staged

A sequence number and a state token do different jobs; we use both.

- **Global `event_log.seq`** is the **ordering + gap-detection** cursor. It must be a total order so contiguity is unambiguous and cross-type causal order (e.g. arrival → counter update) is preserved. Adopted at **P2**.
- **Per-(account, type) state tokens** mirroring `sync_cursor` are the **recovery scope**. On a too-old gap we resnapshot only the diverged `(account, type)` via `POST /read`, not the world. Adopted at **P3**.

This mirrors the backend's existing dual mechanism (global event log + per-type cursors) and composes with B's typed envelope, which already carries `(id, type, account)`. Global-only was rejected (recovery becomes all-or-nothing; couples noisy/quiet accounts on one counter). Per-type-only was rejected (loses cross-type ordering; needs a partitioned stream). The hybrid stages cleanly across P2 and P3.

## Conversation-summary recomputation

The summary is an aggregate over thread messages (`isFlagged = any`, `preview`/`latestMessage`, `messageCount`, `unreadCount`). A per-message diff cannot recompute thread-level aggregates without the parts — which is exactly why `applyHeuristicConversationPatch` returns `incomplete:true` on flag-clear (`mailState.ts:315-345`).

**Short-term, tiered** (mirrors the recovery ladder):

1. **Full `ConversationView` cached → re-fold** (`summarizeConversation`). Exact; only available for an open thread.
2. **Else, heuristic for monotonic-safe fields** — `unreadCount ± 1`, set-`isFlagged`-true — where provably exact.
3. **Else (`incomplete`) → targeted hydrate** of that one conversation summary, *not* a list invalidation.

**End-state under B/P4 (folded into B's object-scoped events).** The server already recomputes the conversation projection on mutation — `refresh_conversation_projection_tx` (`mutations.rs:276`, `projections.rs:39`) — so it has the authoritative aggregate at mutation time. Add a `conversation.updated` event topic carrying that recomputed summary. The client applies idempotently — never re-derives an aggregate from partial parts. Same "authority computes post-state, client applies" principle as the rest of the design; the partial-cache problem dissolves.

## Goals / Non-goals

**Goals.**

- A single governing model for reconciliation — idempotent post-state assertions — replacing the per-topic patch/invalidate matrix.
- Kill the blink by removing the eager `messagesRoot` invalidation as the default reconciliation path.
- Remove the local-echo suppression hack as a *consequence* of idempotent application, not as a special case.
- Make divergence detectable (seq contiguity, state tokens) and recoverable (collapsed catch-up, authoritative resnapshot), with invalidation demoted to the recovery fallback.
- Minimize the client–server API surface to two channels (commands + sync stream) plus asset HTTP; the replica answers reads locally.
- Establish the **replica abstraction** as the uniform layer the UI consumes; feed mechanisms are an implementation detail behind it.

**Non-goals.**

- **Not CRDTs.** Mail has a single authority per account, all writes funneled through `ifInState` optimistic concurrency (`crates/posthaste-engine/src/live.rs:354`, [[L1-jmap]]). Conflict resolution is server-wins + resync, not commutative merge — CRDT metadata buys nothing here.
- **Not an offline-first write queue.** v1 mutations go directly to the server; there is no offline mutation queue ([[L0-sync]] `docs/L0-sync.md:38-40`). Out of scope now.
- **Not a rewrite of React Query.** The replica layers *on top of* the existing `queryClient`/`queryKeys` topology and the domain-named read-model authorities from [[DESIGN-L1-client-read-models]]; it does not replace the cache. (Phase U shifts components from cache queries to replica selectors, but the cache library survives — only its role narrows.)
- **Not a general subscription framework.** The server stays mostly stateless. Server-only filters (full-text search, opaque smart-mailbox rules, future custom scripts) are handled by stateless `query`-style endpoints that return matching IDs; the IDs are reconciled into the replica via normal events. Live demand-based subscriptions may be added *per feature, leaf-level*, scoped to active UI mount, only when measurement shows polling-on-change is insufficient — never as a framework. Server complexity costs more per unit than client complexity, and a generic subscription framework is hard to un-build once it exists.

## Alternatives considered

**Replicache-style cookie + rebase.** Pulls a patch + an opaque cookie (~ our state token) and *rebases* pending un-acknowledged local mutations on top of authoritative server state, discarding speculative results the server did not confirm. Structurally this is our model plus an explicit pending-mutation rebase queue. We omit the rebase queue in v1 because v1 has no offline queue (writes are synchronous to the server); a single in-flight optimistic patch corrected by the authoritative diff is sufficient. Revisit if/when an offline write queue lands.

**Linear-style sync engine.** A local object graph kept current by a delta-sync transaction log, each client tracking a last-synced version cursor. Same cursor+delta+full-snapshot shape we already have server-side; the design adopts the discipline but keeps Posthaste's JMAP-native vocabulary (`*/changes`, state tokens) since the backend already speaks it.

**Convex/Firebase-style stateful subscriptions.** A clean programming model (every query auto-updates), but pays for it always: subscription lifecycle, per-client state on the server, GC, reconnect-state-recovery, per-subscription backpressure, multi-client coordination, and a third wire protocol. The wins (server-pushed updates for server-only queries, ~one round trip saved per relevant change) are real but bounded, and apply to a minority of views. Server complexity is more expensive to operate than client complexity (shared, always-on vs. per-user, restartable). Rejected as the default; per-feature demand-based subscriptions remain available as a future leaf under the replica abstraction.

**CRDTs.** Rejected. CRDTs solve concurrent multi-writer conflict-free merge with no single authority; mail has a single authority per account. Server-wins + resync is the right resolution model; CRDT metadata buys nothing.

**Dedicated `/changes` endpoint.** Initially considered as a second protocol returning collapsed `{created, updated, destroyed}` sets. **Superseded by `/events?collapse=byId`** — same collapse semantics, same envelope, same client apply path. Adding a second protocol would have introduced a second reconciliation code path; the collapse mode achieves identical results with zero new client code.

**Status quo: per-event invalidation.** Rejected — it rots. The matrix forces per-topic decisions with no governing rule; the default-to-invalidate causes the blink and necessitates the echo-suppression hack. Each new optimistic surface adds an asymmetric row. Does not scale with the product.

## Phased rollout

**B leads.** P1 could run against today's correct-by-convention message diffs, but we sequence **B before P1** so the client's event handlers consume the hardened, typed, object-scoped envelope from day one — never wiring against the legacy untyped `json!` shape and reworking. The cost is that the blink persists until B → P1 land; the benefit is no legacy-shaped handler to migrate and no second pass.

### B — Backend: enforce the event-diff contract

*Ships:*
1. A single mutation→event chokepoint — the command path reuses the sync path's pre-image diff (generalize `append_message_diff_events_tx`, `mutations.rs:548-610`) instead of hand-emitting.
2. Typed per-topic payload structs in Rust replacing `json!` literals.
3. Object-scoped update events with full post-state, in a uniform JMAP-shaped envelope (`{type, kind, id, account, fields}`); field-scoped topics (`keywords_changed`, `mailboxes_changed`) collapse into `*.updated`. Coarse-only types (`settings.*`, `account.*` runtime) explicitly documented as such.
4. `asyncapi.json` as generated source of truth for client event types, mirroring `openapi.json` for REST.
5. A convergence property test: replaying a mutation's emitted events onto a fresh replica reproduces server post-state. Build-fail on incomplete diffs.
6. `conversation.updated` event emitted from `refresh_conversation_projection_tx` so the recomputed aggregate ships as a diff (the partial-cache problem dissolves).

*Unblocks:* every subsequent phase. *Risk:* medium — touches the store mutation layer and the event schema; the convergence test is the safety net.

### P1 — Frontend: apply diffs (consume B's envelope)

*Depends on:* B. *Ships:*
- `applyMessageEventPatch` handling the unified `message.updated` topic (and `kind: 'destroyed'` for removal), reusing `applyMailboxPatch`/`applyKeywordPatch`. Same path for live events, optimistic predictions, and catch-up assertions.
- `applyConversationEventPatch` for `conversation.updated` (replaces the heuristic-or-invalidate dance).
- Drop the success-path `messagesRoot` invalidation from `OperationsProvider`.
- Delete `recordLocalMutationEvents` / `shouldSuppressLocalEcho` and the call sites — idempotent apply makes the suppression hack unnecessary.

*Unblocks:* the blink is dissolved; echo-suppression race goes away; one apply path across all event sources. *Risk:* low — the diff exists on the wire (post-B), the work is consolidating handlers and removing now-redundant machinery.

### P2 — Seq gap detection + collapsed catch-up

*Ships:*
- Contiguity check in `useDaemonEvents` (replace the blind `setItem`, `useDaemonEvents.ts:110-113`).
- `/events?afterSeq=N&collapse=byId` backend mode — greatest-seq-per-(account,type,id) walk over the log, streamed as the same envelope. The "now live" cutover marker already exists (`replayed_through`, `api.rs:2766-2811`).
- Surface backend `Lagged(n)` as an explicit gap signal instead of the silent `_ => None` (`api.rs:2803`).

*Unblocks:* self-healing replica within a session, not just across reconnects; bounded catch-up cost (O(changed objects), not O(events)). *Risk:* medium — backend collapse-walk implementation and gap-handling correctness.

### P3 — Per-type state tokens + authoritative resnapshot

*Ships:*
- Client per-(account, type) state tokens mirroring `sync_cursor`.
- Resnapshot path via `POST /read` (or scoped pull) that prunes stale local rows — the client analog of `replace_all_messages` (`mutations.rs:95-130`).
- Invalidation formally demoted to the resnapshot trigger only.

*Unblocks:* the full `cannotCalculateChanges`-equivalent recovery ladder; replica converges to server truth from any divergence. *Risk:* medium-high — client-side token bookkeeping + a pruning pass that must agree with the read-model authority boundary from [[DESIGN-L1-client-read-models]].

### U — Retire HTTP read endpoints; the replica is the source of truth

*Ships:*
- Frontend data layer shifts from REST queries (`useQuery`/`useInfiniteQuery`) to **replica selectors** (`useReplicaSelector(selector)`).
- Per-view derivation logic (filter + sort + window + insertion math from §"Per-view derivation") implemented on the replica.
- Stateless `query`-style endpoint for server-only filters (full-text search, opaque smart-mailbox rules), returning IDs that the replica then ensures are loaded.
- HTTP list/detail endpoints removed (`GET /messages`, `GET /conversations/{id}`, `GET /messages/{id}/detail`, etc.).
- `POST /read` generalized to return the unified assertion envelope (the same shape `/events` delivers), plus a cursor at the end so the client transitions seamlessly into the event stream. Bootstrap and resnapshot both use this one operation, distinguished only by the scope they ask for.

*Unblocks:* the API surface collapses to commands + sync stream + assets. New features inherit the replica abstraction with no per-endpoint plumbing. *Risk:* high (breadth) — frontend rewrite of every data-fetching site. Mitigated by the abstraction: components migrate one at a time; the replica can be fed by both event stream and old endpoints during the transition.

### Future (out of scope of this design)

- **Demand-based per-feature subscriptions** if a specific feature measurably needs live server-pushed updates (e.g. a live full-text search results panel updating as you type). Added as leaves under the replica abstraction — *not* as a framework.
- **Offline write queue** (Replicache rebase pattern). Composes cleanly with the replica when needed.

## Open questions (residual)

- **Sparse replica details.** Does the live `/events` stream stay account-scoped (server filters by account subscription) or fully broadcast with client-side filtering? Lean toward account-scoped at the connection level (matches existing `/events` shape) with client-side row-level filtering. Decide during P1.
- **Convergence-test scope at B.** Which mutation paths and which object types are covered in the property test for v1? At minimum every command in `commands.rs` against the message-object-scoped envelope. Decide during B planning.
- **Snapshot-read response shape.** The existing `POST /read` returns per-endpoint shapes; under U it returns the unified assertion envelope (same as events) plus a cursor for the client to resume the stream. Confirm the migration path and whether old shapes need a transition period during U.

## Security/privacy note

Unrelated to reconciliation but adjacent in rendering: email bodies currently load external stylesheets (Apple SF Pro fonts) at render time — a remote-content leak that exposes recipient activity/IP to a third party on body open, the same vector as tracking pixels. This is a sanitizer concern, not part of this design; flagged as a **separate follow-up** to strip or proxy external `<link rel="stylesheet">`/`@import` in the body sanitizer. Tracked here only so it is not lost.
