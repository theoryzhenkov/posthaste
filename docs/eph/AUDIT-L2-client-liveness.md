---
scope: L2
summary: "Client-liveness audit. PRIMARY (owner's daily pain, §0): steady-state OPTIMISTIC-COVERAGE lag — a user's own mark-read decrements the SMART mailbox counter instantly but the SOURCE folder counter lags ~10-15s, and a draft edit shows in Drafts ~10-15s late. Root cause: the M46/D116 migration moved source-mailbox counts onto the live-store/countDelta channel, but (a) the optimistic fold emits no count delta (count optimism explicitly deferred — 'mutation-id-end-to-end') and (b) the client-mutation echo message.updated carries NO countDeltas/projection (the store command computes them, the service discards them and hand-builds a bare event), while smart counts kept working by accident because they still ride react-query invalidation off the synchronously-written canonical row. Draft edits have no optimistic upsert (M65 fold_effect=None, deferred D132). SECONDARY (recovery, §4): W1 silent-dead-stream watchdog gap, C1 counts have no recovery-edge reconcile. Fix: one coverage model — every message.updated (sync AND optimistic echo) carries projection+countDeltas, and/or optimistic count deltas on the fold."
modified: 2026-07-05
reviewed: 2026-07-05
lifecycle: ephemeral
type: AUDIT
state: evidence-complete
depends: [RFC-L2-client-resilience]
dependents: []
---

# Client Liveness — Foundational Audit (why "nothing updates until reload" keeps coming back)

> **Status: EVIDENCE/AUDIT (evidence-complete).** Read-only investigation of the
> recurring non-liveness class (new messages don't appear, unread counters don't
> move, reload fixes everything) that has survived M40 (stale-link re-prepare)
> and the landed parts of M44 (recovery-edge reconcile, RC1/RC3). Cross-links
> RFC-L2-client-resilience (M40–M50, D112). All paths absolute under
> `/home/usr.prj_posthaste/src/.workspaces/web-tags` (elided below as `R/`).
>
> **[Priority update 2026-07-05]** The owner reframed the actual daily pain: it
> is NOT primarily the silent-stream recovery freeze (W1/C1, kept below as §3–§5)
> — it is a STEADY-STATE lag on the owner's OWN mutations (mark-read source
> counter, draft edit). That is a **distinct** finding, traced fresh in **§0**,
> and it is the bigger UX win. The recovery findings (W1/C1) remain valid but are
> secondary.

## §0. Steady-state optimistic-coverage lag (the primary finding)

**Symptom (owner):** (1) mark-read a message shown in a smart mailbox → the SMART
mailbox unread counter drops instantly, but the SOURCE/physical folder counter
(which also counted it unread) lags ~10-15s; (2) edit a draft → the new version
appears in Drafts only ~10-15s later. Both should be instant.

**Root cause in one line:** the M46/D116 count migration moved *source-mailbox*
counts off react-query onto the live-store slice, which is fed **only** by
server `countDeltas` on the stream — but neither the optimistic fold nor the
client-mutation *echo* event ever produces a `countDelta`, so the source counter
cannot move until a later **sync/flush** cycle re-emits a countDelta-carrying
`message.updated`. Smart-mailbox counts were **not** migrated (still react-query),
so they kept a working live path — a react-query invalidation that refetches the
**synchronously-written canonical row**. The asymmetry is a migration seam, not a
smart-vs-source computation difference.

### A. Mark-read: why smart is instant and source lags

Trace of a client mark-read (`setKeywords` remove `$seen` → via `runMutation`):

1. **Client optimistic fold — updates the ROW, never a COUNT.** The entity-store
   adapter folds `acceptMutation`
   (`R/apps/web/src/runtime/replica/entityStoreAdapter.ts:632-732`) →
   `EntityStore::accept_mutation`
   (`R/crates/posthaste-replica-projector/src/entity_store.rs:162-172`) only
   re-derives the message projection + view membership. **Counts are explicitly
   NOT touched by the fold** — the store's own contract:
   `entity_store.rs:42-45` ("Counts are **not** derived… a count delta from the
   authority is the only path. Optimism for counts is a later concern
   (mutation-id-end-to-end)"). Count scalars move only via `apply_count_delta`
   (`R/crates/posthaste-replica-projector/src/projection.rs:303-307`), fed only
   by ingested `countDeltas` (`entity_store.rs:148-152`). So the fold moves no
   counter — neither smart nor source.

2. **The client-mutation ECHO `message.updated` carries NO countDeltas and NO
   projection.** The mutation reaches the authority server's `set_keywords`
   (`R/crates/posthaste-authority-server/src/authority_server/commands.rs:150-163`)
   → `MailService::set_keywords`
   (`R/crates/posthaste-domain-service/src/service/mutation.rs:153-171`) →
   `queue_then_emit_message_operation` (`mutation.rs:37-91`). That function
   applies the change to the canonical SQLite row synchronously
   (`apply_assertion_to_canonical`, `mutation.rs:61-74, 98-144`) — but it
   **discards** the store command's `CommandResult` (which *does* build
   `countDeltas`: `R/crates/posthaste-store/src/mutations/commands.rs:94-110`;
   the `offload(...).await?` at `mutation.rs:111-114` throws the events away) and
   instead hand-builds its own event `{ messageId, changes:{ keywords:true } }`
   with **no `projection`, no `countDeltas`** (`mutation.rs:75-90`, payload from
   `:165-168`). This is what `publish_events` broadcasts
   (`authority_server/commands.rs:160`).

3. **The client store drops that echo.** `storeUpdateFromEvent` returns `null`
   for a non-deleted event with no projection
   (`entityStoreAdapter.ts:918-925`), so the echo materializes nothing and no
   `countDelta` is applied → the **source** live-store count
   (`useMailboxCounts`, `R/apps/web/src/components/sidebar/SidebarItems.tsx:119-120`,
   `SourceSection.tsx:61-68`; slice `R/apps/web/src/live-store/store.ts:113-132`)
   does not move.

4. **Smart mailboxes ride a different, still-live channel.** The same echo drives
   the M47 boundary handler (`R/apps/web/src/domain-cache/handlers.ts:130-161`):
   the `keywords` branch calls `invalidateMailboxReadModels`
   (`:153-161`) which invalidates **`queryKeys.smartMailboxes`**
   (`R/apps/web/src/domain-cache/invalidations.ts:106`) → the `smartMailboxes`
   query refetches (`R/apps/web/src/mailboxNavigationReadModels.ts:161-163`) →
   reads the **server-computed** unread from the canonical row that step 2 already
   wrote synchronously → the sidebar's `smartMailbox.unreadMessages`
   (`R/apps/web/src/components/sidebar/SidebarContent.tsx:86`) updates in ~one
   local round-trip = "instant." **Crucially, the same `invalidateMailboxReadModels`
   SKIPS `queryKeys.mailboxes(accountId)`** — the source-folder counts —
   `{ skipStoreOwned:true }` (`invalidations.ts:101-105`), because "the store owns
   counts." So the source counter gets neither a countDelta (step 3) nor a
   refetch. Nothing updates it.

5. **The source counter finally moves on the next SYNC/FLUSH.** The mark-read
   triggers an immediate follow-up sync
   (`trigger_outbox_flush` → `SyncTrigger::Manual`,
   `authority_server/commands.rs:135-148, 161`). The **sync apply path** —
   distinct from the optimistic echo — emits `message.updated` **with** projection
   + countDeltas (`R/crates/posthaste-store/src/mutations/message_apply.rs:45-54`
   → `projection_tracking.rs:72-106`, projection at `:98-100`, countDeltas at
   `:104`). That event the client store *does* ingest → `writeMailboxCount`
   (`entityStoreAdapter.ts:1004-1006, 1067-1083`) → source counter drops. The wait
   is that sync's connect + provider round-trip.

**Confirm/refute the hypothesis:** *Refined, partly refuted.* Both smart and
source counts are **server-authoritative** (smart is not "client-computed from
the entity store"). The real asymmetry is the **delivery channel**: smart counts
still ride react-query invalidation (which refetches from the synchronously-
written canonical row → fast), while source counts were migrated to the
live-store/countDelta channel — and **the optimistic path (fold and echo) emits
no countDelta**, so source counts are stranded until a sync re-emits one.
Confirmed: the fold applies no source-mailbox count delta; confirmed: the echo
event omits countDeltas/projection; confirmed: the counts query is deliberately
not invalidated for source (`skipStoreOwned`).

### B. Draft-edit lag

Confirmed as the coordinator described. `SaveDraft` is routed through
`runMutation` with **no optimistic fold** (M65 `fold_effect=None` — "the fold
vocabulary can't express an upsert":
`authority_server/commands.rs:730-738`; client note
`R/apps/web/src/runtime/mutations.ts:236-238`). The authority `save_draft`
**only enqueues an outbox op and triggers a flush — it publishes NO event
synchronously** (`authority_server/commands.rs:267-276`; service side just
`queue_operation`, `R/crates/posthaste-domain-service/src/service/outbox.rs:351-397`).
Contrast `discard_draft` (D130), which *does* `publish_events` immediately
(`commands.rs:294-303`) — that is why discard blinks instantly but save does not.
The draft's optimistic existence lives **only in the outbox, with no projection
row** (RFC-L2-draft-identity:164-166). So the Drafts list updates only when the
flush pushes the draft to the provider and the resulting create/update is
observed and re-emitted as the D132 reconciling `message.updated` — i.e. on the
**flush/sync**, NOT on save-settlement. The M65 worker deferred exactly this as
"a latency cost, reconciles via sync."

### C. What the ~10-15s actually is

It is **the triggered follow-up sync's flush + provider round-trip**, not the
poll interval. Both A(5) and B fire `trigger_outbox_flush` →
`trigger_account_sync(SyncTrigger::Manual)` immediately
(`authority_server/commands.rs:135-148`); the wait is that sync acquiring a slot
from the global concurrency governor (`sync_flow.rs:163`), connecting, pushing
the op, re-observing the provider, and emitting the countDelta/projection-carrying
`message.updated`. The 60s `poll_interval` default
(`R/crates/posthaste-config/src/daemon.rs:97`) is only the **fallback ceiling**
if the triggered flush fails. So the user is waiting on **one IMAP/JMAP
connect+round-trip**, typically several-to-~15s — not a fixed timer. (Other loops
nearby for reference: `CACHE_WORKER_INTERVAL` 2s, `AUTOMATION_BACKFILL_INTERVAL`
15s, `R/crates/posthaste-authority-server/src/supervisor/types.rs:5-13` — none of
these drive the count; the flush-sync does.)

### D. Fix direction (one coverage model, not a patch)

The principle: **every `message.updated` must carry the same enriched payload
(projection + countDeltas) regardless of origin**, so the client entity store is
the single coverage model for counts — sync events and optimistic-echo events
alike. Today the sync path is enriched and the optimistic-echo path is not; that
split IS the bug.

- **A — two layers, do the first now:**
  1. *Stop discarding the countDeltas the store already computes.* The
     `MessageCommandStore::set_keywords`/`replace_mailboxes` `CommandResult`
     already contains a projection+countDelta-carrying event
     (`store/mutations/commands.rs:94-110`, `write_store.rs:20-26`); route THAT
     event through `publish_events` instead of hand-building a bare
     `{changes:{keywords:true}}` one in `queue_then_emit_message_operation`
     (`mutation.rs:75-90`). The canonical row is already written synchronously,
     so these counts are correct-now. This alone collapses the source-counter lag
     from ~10-15s to **sub-second** (local echo), with no new optimism machinery —
     and it unifies the echo with the sync path (one payload shape). This is the
     recommended immediate fix.
  2. *(Optional, for true zero-round-trip instant)* implement the deferred
     "optimism for counts (mutation-id-end-to-end)" the store contract names
     (`entity_store.rs:44`): on the fold, apply an optimistic ±1 unread delta to
     each of the message's source mailboxes, settled/reverted with the mutation
     like row optimism. Needed only if (1)'s sub-second echo is still judged too
     slow; (1) is the 90% win.
  - *Also correct the seam:* once (1) lands, the smart-vs-source asymmetry
     disappears at the source (both move on the echo); the `skipStoreOwned` skip
     of `queryKeys.mailboxes` becomes correct rather than lag-inducing.

- **B — the D132 optimistic draft upsert is the right fix, and the draft-identity
  refactor already scopes its prerequisite.** The blocker is real (the fold
  vocabulary has no upsert). But RFC-L2-draft-identity's stable `draft_key` +
  `draft_registry` (M63–M69, esp. the M69/D135 sync write-through) is exactly the
  identity that makes an optimistic draft-upsert *reconcilable*: materialize the
  saved draft version into the entity store immediately under the stable key, and
  let the D132 reconciling `message.updated` (flush-observed, provider id rotated)
  converge it. That is a bounded fold-vocabulary extension (add a draft-upsert
  assertion) done *principled* under the draft-identity model — not an ad-hoc
  Drafts-list patch. Recommend it be scheduled as the "deferred D132 optimistic
  upsert" follow-up the M65 worker explicitly parked, gated on M69 landing.

**Classification:** A(1) and B are bounded gaps that restore the *single coverage
model the M46/D116 migration intended* (every count update rides projection+
countDeltas on the stream). Neither is a redesign; both close a migration seam.

### E. §0 vs W1/C1 — which is the bigger UX win

**§0 is the bigger win.** It fires on **every** ordinary interaction the owner
performs (mark-read, move, draft edit) in the **healthy steady state**, on
**every** session — a guaranteed multi-second lag on the most common actions.
W1/C1 fire only on a **failure edge** (a silently-dead socket / a missed-event
recovery gap) that many sessions never hit. §0 also has the cheaper fix: A(1) is
a server-side event-payload change (publish the countDeltas the store already
computes) with no new client machinery, versus W1's engine watchdog. Recommend
sequencing A(1) first (highest value / lowest cost), then B (gated on the M69
draft-identity work), then W1, then C1.



**The architecture is NOT snapshot-on-open + periodic-resync.** There is a real,
continuous push of new-message rows *and* count deltas during steady state: every
sync-applied message emits a `message.updated` domain event carrying the full
`projection` + absolute `countDeltas` (`firehose-carries-rows`,
`counts-on-the-stream`), the runtime forwards every one to every open client link
as a `Notification` frame, and the client's WASM entity store places new rows and
updates counts, notifying React through the M46 live store. Path A and Path B are
correctly wired hop-for-hop in the bundled topology — new-message arrival is live
*by design*.

**The foundational culprit is the layer AROUND that push: link liveness and
recovery are edge-triggered only.** The whole client's liveness hangs on one
long-lived SSE stream, and the client only heals when it *observes an error
event*. Two concrete manifestations:

1. **W1 — no stream-liveness watchdog (the silent-dead link).** The near-end
   engine detects stream death only via explicit `Error`/`Closed` events. A
   half-open TCP connection (laptop sleep/wake, NAT rebind, Wi-Fi/VPN switch)
   produces *no event*: `stream.next().await` blocks forever, no reconnect, no
   M44 recovery edge, no error — the client is frozen until reload. The server
   already sends SSE keep-alives every 15s; the engine explicitly *ignores* them
   instead of using their absence as a liveness deadline.
2. **C1 — counts have no level-triggered recovery, at all.** Rows self-heal on
   every view re-open/reconnect (the snapshot re-serves them). Counts do not:
   they update ONLY from live `countDeltas`; the M44 reconcile does not touch
   them (RFC D112 "RC2 / counter refetch on reconcile (D113)" is the still-
   pending half of M44); the REST invalidation of the counts query is
   deliberately disabled ("store-owned"); and once a live count is seeded it
   permanently shadows the server count in the sidebar. **Any** missed event —
   a reap gap, a sleep gap, W1 — freezes that mailbox's counter until reload.

**Redesign vs bounded gap: honestly, bounded gaps — but gaps in the one place
the design says must be level-triggered.** The subscription/push model itself is
sound and does not need redesigning. What's missing is the self-healing layer
RFC-L2-client-resilience already prescribes (D112's "level-triggered,
always-correct" reconcile) applied to *all* live state (counts, not just view
rows) plus one property the RFC never named: the stream itself must be
liveness-checked, because every recovery mechanism in the system is currently
parked behind "the stream reports an error", and there is a common real-world
failure where it never does.

Ranked causes (§4): **W1 ≥ C1** (each alone reproduces a reported symptom;
together they explain "recurs despite M44") **> R1** (adapter-install race —
intermittent full-session deadness) **> T1** (split-runtime topology is
structurally not-live — only if `[link] authority_server_url` is set) **> the
300s reap** (no longer a root cause after M40/M44, but it multiplies C1's
exposure).

---

## 1. Path A — a new message arrives on the server → the open mail list

### 1.1 Server: emission during steady-state sync — CORRECT, continuous

- The sync cycle publishes events **per group, as the sync produces them**, not
  at the end: `R/crates/posthaste-authority-server/src/supervisor/sync_flow.rs:221`
  (`let mut publish = |events| shared.publish_events(events)` handed into
  `sync_account_with_mode`), broadcast at
  `R/crates/posthaste-authority-server/src/supervisor/shared.rs:116-120`.
- Every message apply (new or changed) in a sync batch goes through
  `apply_message_record_tx` → `append_message_diff_events_tx`:
  `R/crates/posthaste-store/src/mutations/sync_batch.rs:188`,
  `R/crates/posthaste-store/src/mutations/message_apply.rs:23-55`.
- The emitted event is **`message.updated` (there is no `message.created`
  topic)** with `created: !before.existed`, `changes.arrived`, the **full
  `MessageSummary` projection** and the affected mailboxes' **absolute counts**
  (`countDeltas`, read in-tx):
  `R/crates/posthaste-store/src/mutations/projection_tracking.rs:72-106`
  (projection attached at :98-100, countDeltas at :104). Deletes/expunges also
  carry projection+counts (`:198-215`, `sync_batch.rs:147-155`).

So step 1 answers: yes, a new-message-arrival event is emitted **on every
steady-state sync application**, projection-carrying, count-carrying. No trigger
gating.

### 1.2 Runtime → client link: delivery — CORRECT (bundled topology)

- In the bundled (default) topology the runtime shares the authority server's
  event bus (`R/crates/posthaste-server/src/startup.rs:27-33` — `InProcess`
  unless `[link] authority_server_url`;
  `R/crates/posthaste-runtime/src/assembly.rs:241-243` — "In-process this is the
  authority server's bus").
- Each open client link spawns a **notification forwarder** over that bus which
  forwards EVERY scope-matching domain event as a
  `RuntimeFrame::Notification { kind: topic, payload: <the whole DomainEvent,
  camelCase> }`:
  `R/crates/posthaste-runtime/src/far_end/links.rs:829-866` (forwarder),
  `:883-904` (`forward_notification`), scope check `:1210`.
  Bus lag is recovered by collapse-to-snapshot (`:843-860`), broadcast-channel
  lag by collapse too (`subscribe_frames`' stream, `:320-346`) — gap handling on
  this hop is sound.
- Mail-list **view frames** are intentionally NOT re-served per event for
  self-maintained views ("option iii"): the view registry's event pump skips
  recompute when `descriptor.client_self_maintained`
  (`R/crates/posthaste-runtime/src/far_end/view_registry.rs:199-243`, skip at
  :229-232). The client stamps that flag from the same predicate derivation the
  WASM store uses (`R/apps/web/src/runtime/httpAdapter.ts:136-148`,
  `R/apps/web/src/runtime/mailListSelfMaintained.ts:57-96`), so deferred views
  (user smart mailboxes, search, non-date sorts) still get per-event
  `ViewReplace`/`ViewDelta` re-serves (`view_registry.rs:231-232`,
  `links.rs:1085-1116`). **New rows for the open mailbox travel on the
  notification firehose, not on view frames — by design.** The transport is SSE
  with axum keep-alives every 15s
  (`R/crates/posthaste-http-api-adapter/src/api/runtime_stream/links.rs:106`).

So step 2 answers: an open message-list view does **not** receive incremental
new-row *view* events — and doesn't need to; the continuous `message.updated`
firehose carries the new row (projection) and the client store places it. The
push is continuous, not periodic.

### 1.3 Client: frame → WASM store → live store → React — CORRECT (when subscribed through the entity-store adapter)

- Engine → host: frames arrive via the near-end WASM engine's `onFrame`
  (`R/apps/web/src/runtime/nearEnd.ts:241-249`) → the one shared subscription in
  `linkClient.ensureStream` (`R/apps/web/src/runtime/linkClient.ts:103-169`) →
  `runtimeStream.subscribe` → the **entity-store adapter's** wrapped handlers
  (`R/apps/web/src/runtime/replica/entityStoreAdapter.ts:522-555`).
- `message.updated` notifications are coalesced (`routeFrame`, `:569-600`;
  rAF flush with a 256-frame sync cap, `:128-143`) and folded into ONE
  `ingestBatchJson` (`flushPendingFrames`, `:602-630`). Projection-carrying
  events materialize the message and apply `countDeltas`
  (`storeUpdateFromEvent`, `:905-933`).
- The WASM store places an in-coverage new row at the top of a desc view and
  marks view + mailbox dirty
  (`R/crates/posthaste-replica-projector/src/entity_store.rs:19-47` — the
  place-or-ignore contract over `[TOP, W]`; verified by its
  `in_range_arrival_is_placed_at_top_of_desc_view` test, `:346`).
- `drainAndEmit` (`entityStoreAdapter.ts:987-1008`) re-projects dirty views,
  mirrors rows into the live store (`setViewProjection`, `:1028`) **and**
  synthesizes a `viewReplace` into the sink (`:1030-1036`) — which is what the
  mail list actually renders today (M49 residual): `useRuntimeMailListView`
  consumes viewSnapshot/viewReplace/viewDelta into the query cache
  (`R/apps/web/src/components/message-list/useRuntimeMailListView.ts:214-280`).
- The live store notifies `useSyncExternalStore` subscribers on every real
  change with stable-reference dedupe
  (`R/apps/web/src/live-store/store.ts:77-86` rows, `:113-132` counts,
  `:162-183` hooks). No missing-notify gap here: producers always `emit()` when
  the value moved.

So step 3 answers: a delivered event reaches the store, mutates the projection,
and notifies React. **The steady-state pipeline has no wiring hole** — matching
M44's own finding ("Steady-state was CLEARED", RFC-L2-client-resilience M44 row).

## 2. Path B — the unread counter

- Counters are **not** react-query state (D116/M46): the sidebar reads the live
  store's counts slice via `useMailboxCounts`
  (`R/apps/web/src/components/sidebar/SidebarItems.tsx:119-121`,
  `SourceSection.tsx:61-68`), **falling back** to the mailboxes query's server
  count only while no live entry exists ("bootstrap seeding").
- Live updates: `countDeltas` on the same `message.updated` batch →
  `writeMailboxCount` → `setMailboxCount`
  (`entityStoreAdapter.ts:1004-1006, 1067-1083`). The deltas are **absolute
  per-mailbox counts** read in-tx at emission (`projection_tracking.rs:101-104`)
  — so one later event fully heals a stale count *for that mailbox*.
- The M47 event boundary does have a `MessageUpdated` handler
  (`R/apps/web/src/domain-cache/handlers.ts:130-175`) — it is NOT a missing
  no-op like `account.status_changed` (`:108-110`) — but it **unconditionally
  skips the counts query** (`const skipStoreOwned = true`, `:136`;
  `invalidations.ts:93-108` skips `queryKeys.mailboxes(accountId)`), because the
  entity store owns counts. `sync.started` invalidations skip it too
  (`invalidations.ts:36-47`).
- **Therefore counts are edge-triggered ONLY, with three compounding properties:**
  1. no reconcile on the recovery edge — `reconcileOpenViews`
     (`linkClient.ts:199-220`) re-opens views (RC1) and blips health; it does
     NOT refetch/reseed counts (the RFC's RC2 / M44-old "counter refetch on
     reconcile (D113)" — still pending);
  2. no REST fallback — the store-owned invalidations are disabled by design
     (the M46 gate);
  3. no self-correction via the query — even when react-query's
     `refetchOnWindowFocus` refreshes `mailboxes(accountId)` with correct server
     counts, the sidebar still prefers the (stale) live entry: the live slice
     "takes over" permanently once seeded (`SidebarItems.tsx:115-120`).
  Re-opened view snapshots also seed rows with `countDeltas: []`
  (`entityStoreAdapter.ts:218-229`), so a view re-open never repairs counts.

**Path B verdict:** the counter is live in steady state, but a single missed
`countDeltas` (any stream gap) freezes it until *another* event touches the same
mailbox — or a reload (which resets the live slice and re-runs bootstrap
fallback). This is exactly "counters don't move until reload."

## 3. The recovery machinery, and where it stops short

- **M40 (landed):** a 404/410 on the stream GET re-prepares a fresh link instead
  of halting (`R/crates/posthaste-link-near-end/src/engine.rs:134-162, 439-459`).
- **M44 (landed: RC1/RC3 + visibility):** the fresh-link edge surfaces as
  `onLinkReestablished` (`engine.rs:396-406`, `nearEnd.ts:263-274`), linkClient
  adopts the id and re-opens registered views
  (`linkClient.ts:185-220`), mail-list views register re-open
  (`useRuntimeMailListView.ts:137-144`); a hidden→visible transition reconciles
  defensively (`linkClient.ts:222-239`).
- **Reap:** `SESSION_IDLE_TTL = 300`s reaps a link only when its SSE
  down-stream is gone (`receiver_count() == 0`)
  (`R/crates/posthaste-runtime/src/far_end/links.rs:43, 406-444`); a connected
  stream is never reaped. Post-M40/M44 the reap itself recovers (rows re-serve;
  frames lost in the gap are replaced by the re-served base) — **except counts
  (C1)**, which the reconcile never re-serves.
- **What has NO recovery path at all — W1:** every one of these mechanisms is
  armed by an *observed* stream event. The engine's frame loop
  (`engine.rs:378-481`) reacts to `Open/Message/Closed/Error` and otherwise
  awaits forever; keep-alive blocks are deliberately skipped with no deadline
  armed (`engine.rs:483-488`; browser shim `nearEnd.ts:102-139`,
  `openWhenHidden: true`, no read timeout — `fetch` has none). A half-open
  socket therefore looks identical to a quiet mailbox. Meanwhile the server side
  errors on its next keep-alive write, drops the subscription, and 300s later
  reaps the link — but the client never learns any of it. The visibility
  reconcile (`linkClient.ts:230-238`) partially masks this for backgrounded tabs
  (view re-opens are fresh HTTP POSTs and succeed until the reap; after the
  reap they 404), but a tab that stays visible through a network transition gets
  nothing. This is the "works only after reload" shape M44 explicitly did not
  cover: M44 fires on the *re-prepare* edge, and the re-prepare requires the
  stream to fail observably first.

## 4. Root cause, ranked

| # | Layer | Finding | Evidence | Symptom coverage |
|---|-------|---------|----------|------------------|
| **W1** | Client link (near-end engine) | **No stream-liveness watchdog.** Liveness of the entire client rests on one SSE; failure detection is edge-triggered; a silent-dead socket = indefinite total freeze (rows AND counts), no recovery edge ever fires. Server keep-alives (15s) already provide the heartbeat; the engine ignores them. | `engine.rs:378-481, 483-488`; `nearEnd.ts:102-139`; `runtime_stream/links.rs:106` | Both symptoms, whole-client, until reload. Explains recurrence *despite* M44 (M44 needs an observed error). |
| **C1** | Recovery scope (D112-RC2 unimplemented) | **Counts are edge-only with a permanent live-shadow.** No reconcile on the recovery edge, REST invalidation disabled by design, live entry shadows fresh server counts forever, view re-open reseeds rows but not counts. | `linkClient.ts:199-220`; `handlers.ts:130-175`; `invalidations.ts:93-108`; `SidebarItems.tsx:115-121`; `entityStoreAdapter.ts:218-229`; RFC M44-old row ("counter refetch on reconcile (D113)" — pending) | "Counters don't move" surviving every gap M44 *does* recover. |
| **R1** | Adapter composition (bounded race) | The one shared frame subscription binds `getRuntimeAdapter()` **once**; `installEntityStoreAdapter()` is async and un-awaited. If the subscription (or a view open) wins the race against the WASM/worker install, the session's frames bypass the store entirely: no ingest, no counts, no synthesized viewReplace — full non-liveness for the session, healed by reload (which rewins the race). Nothing re-binds on install. | `adapter.ts:139-201` (fire-and-forget install), `linkClient.ts:103-169` (one-shot `ensureStream`), `entityStoreAdapter.ts:522-555` | Intermittent whole-session deadness on cold cache / slow devices. |
| **T1** | Topology (split runtime only) | In the remote-runtime topology, the authority-server down-channel republishes `message.updated` **without projection or countDeltas** (`down_assertion_to_event`), and the client store drops projection-less events (`!deleted && !projection → null`). Split-mode steady state is **structurally not-live** for new rows and counts. | `R/crates/posthaste-runtime/src/read.rs:378-396`; `entityStoreAdapter.ts:918-924`; default is bundled (`startup.rs:27-33`) | Total, deterministic — but only when `[link] authority_server_url` is configured (dogfood split, `posthaste-runtimed`). |
| 5 | Far-end lifecycle | `SESSION_IDLE_TTL = 300`s: after M40/M44 the reap is recoverable for rows; it remains a *frequency multiplier* for C1 (every reap gap can drop countDeltas that nothing reconciles). Not a root cause on its own anymore. | `links.rs:43, 406-444` | Amplifier. |

Explicit answers to the three foundational questions:

1. **Continuous push vs snapshot-on-open?** Continuous push, by design and in
   code (bundled topology): rows + counts ride the `message.updated` firehose;
   the client store self-maintains membership; snapshots are only bootstrap and
   gap recovery. The "structurally not-live" description is true only of the
   split topology (T1).
2. **M44 in disguise?** Partially. The reap/reconnect family is real but M40+M44
   now recover *rows* from it. What survives M44 is (a) the counts half of the
   reconcile that M44's own spec listed and that never landed (RC2/D113 → C1),
   and (b) the silent-dead stream that never produces the edge M44 listens for
   (W1).
3. **The single culprit layer?** The **link/recovery layer** — the near-end
   engine's liveness detection plus the reconcile's scope. NOT the server (it
   emits, continuously, with full payloads), NOT the subscription model (the
   firehose covers new rows), NOT the WASM store (it places and notifies), NOT
   the reactive mirror or React (notify wiring is correct).

## 5. Design-level fix direction

The correct architecture is the one already half-built: **one continuous
subscription pushing all relevant deltas (rows + counts) into one reactive owner
(the WASM store → live store), wrapped in a level-triggered convergence loop
that assumes the stream can silently lie.** Concretely, in order of leverage:

1. **W1 — a liveness deadline in the near-end engine (new invariant: "a live
   link is one that has produced bytes recently").** The server already
   heartbeats every 15s; the engine should arm a read-deadline (~45s = 3
   missed keep-alives; the host shim must surface keep-alive comments as a
   liveness tick, or the engine can use its existing `Scheduler` to race a
   timeout against `stream.next()`), and on expiry classify as a transient
   stream error — which drops it into the *existing* M40 re-prepare + M44
   reconcile machinery. This converts every silent failure mode into the one
   edge the system already knows how to heal. Engine-owned (policy lives in
   `posthaste-link-near-end`, per D41), config alongside `request_deadline`.
2. **C1 — finish D112: make the recovery reconcile cover ALL live state, and
   remove the live-shadow trap.** Two complementary moves:
   - *Reconcile the counts slice on every recovery edge* (link re-establish,
     reset, tab-foreground): refetch `mailboxes(accountId)` and write the
     server counts **into the live store slice** (`setMailboxCount`), not just
     the query cache — the slice is the owner, so reconciliation must write the
     owner. (This is M44-old's "counter refetch on reconcile (D113)".)
   - *Or better, structurally:* let the re-served base carry counts — include
     current mailbox counts in the mail-list view snapshot (or a tiny
     `mailboxCounts` view re-opened with the others), so the same RC1 re-open
     that heals rows heals counts, with no REST special case. Either way, the
     invariant to enforce: **every piece of live state must have a
     level-triggered re-serve path, not only an edge-triggered delta path.**
3. **R1 — make adapter composition race-free.** Either await
   `installEntityStoreAdapter()` before the first `openRuntimeLink`/subscribe
   (simplest: gate `ensureLink` on the install promise), or make
   `installEntityStoreAdapter` re-bind the active frame subscription when it
   swaps `activeRuntimeAdapter`. One-line invariant: the shared stream must
   always terminate in the entity-store controller.
4. **T1 — if/when the split runtime is dogfooded:** the runtime near node must
   re-derive `projection` + `countDeltas` when republishing down-channel
   assertions (it has `AuthorityServerApi` read access:
   `current_summary`/`query_mail_page`), or the authority-server link's `Base`
   frames must carry them. Until then, note in
   DESIGN-L2-deployment-topology that split mode does not satisfy the client's
   liveness contract.
5. **Non-fix:** raising `SESSION_IDLE_TTL` or adding more visibility-triggered
   refetches would only shrink the window; both leave the class alive. Ad-hoc
   REST re-invalidations on message events (undoing `skipStoreOwned`) would
   reintroduce the refetch storms M46 removed — the reconcile belongs on the
   recovery edges, which are rare and bounded.

**Classification:** W1 + R1 are bounded gaps (a missing watchdog, a missing
await/re-bind). C1 is a *scoped* piece of the existing D112 design that was
specified but never implemented — also bounded, but it should be landed as the
completion of M44 rather than a new patch, because it is the second half of the
level-triggered convergence loop the RFC already committed to. No
subscription-model redesign is warranted.

## 6. Cross-references

- **RFC-L2-client-resilience** — F1 (the original reap-freeze), D112 (one
  reconcile pass; RC1/RC2/RC3), M40 (landed), M44 (landed RC1/RC3; RC2 — the
  counter reconcile, a.k.a. M44-old/D113 — outstanding; this audit's C1), M46
  (the live store this audit traces), M47 (the event boundary checked in §2).
- **W1 has a sibling finding on the provider side:** AUDIT-L2-jmap-push PP1
  ("silent push death") — the same edge-triggered-only liveness assumption, one
  layer down. The watchdog fix shape is the same.
- Adapter/store contracts: `docs/eph/PLAN-L2-client-link-unification`,
  DESIGN-L2-deployment-topology (T1).
