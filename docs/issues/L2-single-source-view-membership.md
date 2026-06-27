---
scope: L2
summary: "Evaluable mail-list views have TWO membership sources — the client store's self-maintenance from message.updated AND the runtime's full-view re-serve (set_view_rows) — which is the dual-source code smell behind the move/delete flicker. Retire the REDUNDANT slice (the runtime re-serving on incremental membership for active-store evaluable views) so the firehose is the single source of truth for in-window membership; keep re-serve only for what the store genuinely can't self-derive (open/page/resync/deferred/store-off)."
modified: 2026-06-27
reviewed: 2026-06-27
lifecycle: ephemeral
type: ISSUE
status: open
priority: medium
depends:
  - path: docs/eph/DESIGN-L2-client-link-reactive-store
  - path: docs/eph/PLAN-L2-client-link-unification
---

# Single source of truth for evaluable-view membership

**Status: OPEN — architecture debt / refactor, not a live bug.** The live bug it
underlies (the move/delete membership clobber) is fixed by the `set_view_rows`
reconcile ([[L2-reserve-clobbers-optimism]]); this issue is the deeper cleanup
that reconcile is a stepping-stone toward.

## The smell: two membership sources for one view

An evaluable mail-list view (`InMailbox`/`All`) gets its row membership from
**two** mechanisms that can disagree:

1. **Client store self-maintenance** — `ingest_batch` / `rederive_message` from
   `message.updated` firehose events (`crates/posthaste-link-replica`). Incremental,
   version-guarded, correct.
2. **Runtime full-view re-serve** — `viewSnapshot` / `viewReplace` →
   `set_view_rows`. The runtime recomputes the whole view and ships the row list.

The reactive store was **layered on top of** the pre-existing runtime
view-serving path rather than replacing it, so both run. For an evaluable view
they compute the *same* membership; when the re-serve is stale it clobbers the
store's correct membership (the flicker). The reconcile makes the client robust
to that, but the **dual source remains** — redundant computation + a standing
conflict the client has to defend against. That's the smell.

## What's redundant vs. load-bearing

The re-serve path is **not** purely legacy — it is the *only* source for things
the partial-window store cannot self-derive. Be precise about the target:

| Re-serve trigger | Status | Why |
| --- | --- | --- |
| Initial snapshot (open) | **keep** | store has zero messages on open |
| Pagination / window-extend | **keep** | a message below the watermark `W` is outside the store's world |
| Resync / gap recovery (forwarder lag → collapse) | **keep** | the recovery path; the firehose dropped events |
| Deferred (smart-mailbox) views | **keep** | store can't evaluate the predicate |
| ~~Store-off fallback (`VITE_ENTITY_STORE=false`)~~ | **RETIRED** | the opt-out is gone (step 1); the store is the sole read model |
| **Incremental membership (move in/out within window) for an evaluable view while the store is active** | **RETIRE** | the store already maintains it from `message.updated` |

Only the last row is the redundant slice. The radical change is *"the runtime
stops recomputing-and-serving full views on incremental membership for
active-store evaluable sessions, and trusts the client to self-maintain from the
firehose"* — not "delete `set_view_rows`."

## The delta spectrum

- **(i) today:** runtime serves full `viewReplace` on every change; store
  re-derives + reconciles. (The adapter opens views *non-delta-capable*.)
- **(ii) deltas:** runtime serves `viewDelta`; store applies incrementally.
- **(iii) target:** runtime serves *nothing* on incremental membership; the
  firehose (`message.updated`) is the single membership channel; the runtime
  serves only open/page/resync/structural/deferred.

(iii) is the single-source-of-truth end-state.

## Why it's worth doing

- **Single source of truth / one channel** — membership flows through one path
  (firehose → store), removing the dual-source conflict at the source rather than
  guarding against it client-side.
- **Perf** — kills the O(view) runtime recompute + serialize + ship on every
  flag/move (the link-bus recompute pattern, [[link-bus-perf-regression]]); the
  client already has the data.
- **Completes the migration** — the reactive store finally *replaces* the legacy
  view-serving for evaluable views instead of shadowing it. Aligns with the
  assertion/delta-based link in [[client-link-unification]].

## Why NOT yet — the tradeoff to respect

The re-serve isn't only redundant computation; for an active store it's also a
**periodic correctness backstop** — the runtime re-asserting authoritative
membership. (iii) removes that net: any gap in the store's self-maintenance has
nothing to correct it until the next resync. We just fixed **four** bugs in that
self-maintenance (keyword-order absorption, early-retire, stale-version, the
membership clobber — see [[L2-reserve-clobbers-optimism]], the flicker arc), so it
is *newly* correct, not *battle-tested*. Retiring the backstop immediately after
finding four bugs in the thing it backs up is premature.

## Prerequisites + migration plan

1. **The reconcile invariant (DONE).** `set_view_rows` reconciles to "a row is
   present in an evaluable view iff its folded base matches the predicate," so the
   client is authoritative + stale-re-serve-proof. This is what makes (iii) *safe*
   to approach — the client no longer depends on the runtime being right about
   membership.
2. **Dogfood clean.** Let the flicker fixes (nightly ≥ .20) ride for a while with
   no membership-flicker reports — evidence the store's self-maintenance is
   trustworthy.
3. **Harden gap-detection.** (iii) leans entirely on the firehose + resync; the
   notification-forwarder-lag → collapse → re-serve path (`2d`) must be airtight,
   since it becomes the *only* corrector.
4. **Negotiate the mode.** A session signals "store-active, self-maintaining" so
   the runtime suppresses incremental-membership re-serves for it (and still
   serves open/page/resync/structural/deferred + the store-off path unchanged).
   This is the runtime-side change — view registry + recompute/serve triggers +
   the delta-capability handshake.
5. **Measure the perf delta** to confirm the recompute saving.

Scope: cross-cutting (runtime view machinery + the link handshake), so a
deliberate effort under [[client-link-unification]] — not a flicker patch.

## Call-site map (2026-06-27)

Every point where the runtime pushes a view frame to a session, mapped against
KEEP vs the redundant target. All frames originate as a `ViewFrame` and are
converted by `view_frame_to_runtime` (sessions.rs) into
`ViewSnapshot`/`ViewReplace`/`ViewDelta` (delta-capable sessions get `ViewDelta`
for row-local mail-list changes).

| # | Site | Frame | Trigger | Verdict |
| --- | --- | --- | --- | --- |
| 1 | `open_view` → `subscribe_view.catch_up` (views.rs:113, 205) | Snapshot | a view is opened | **KEEP** (initial) |
| 2 | `extend_view` (views.rs:187) | Replace | client window-extend (pagination) | **KEEP** (page) |
| 3 | `spawn_event_pump` → `send_recomputed_replace` (views.rs:237, 270) | Replace/Delta | **every affecting `message.updated`** | **REDUNDANT — the target** |
| 4 | `spawn_event_pump` lag → `send_recomputed_snapshot` (views.rs:280) | Snapshot | per-view event-bus lag | **KEEP** (resync) |
| 5 | `subscribe_view` lag (views.rs:217) | Snapshot | view subscription lag | **KEEP** (resync) |
| 6 | `collapse_session_frames` (sessions.rs:730) | Snapshot | notification-forwarder lag | **KEEP** (session resync) |

### The target (#3) in detail

Each open view spawns `spawn_event_pump` — a tokio task subscribed to the domain
event bus. On each event, `event_affects_view(kind, event)` gates a recompute;
for `ViewKind::MailList` it fires on `message.updated` whose `changes.keywords`,
`changes.mailboxes`, `created`, or `deleted` is set. A hit calls
`recompute_view_if_changed` → `build_snapshot` (a **full `query_mail_page`
re-query of the store** + serialize) → emits `ViewFrame::Replace` if the data
changed. So **every membership/keyword/arrival/deletion event triggers a full
view rebuild per open view** — O(open-views × view-size) of store query + serialize
per mutation/sync event. That recompute cost is the perf prize.

### Why it's redundant — and the dependency that gates removal

The **same event bus** also feeds `spawn_notification_forwarder` (sessions.rs),
which forwards the raw `message.updated` as a `RuntimeFrame::Notification`. So
the client receives *both* the recomputed view Replace/Delta (#3) **and** the raw
notification. The **entity-store adapter** (`entityStoreAdapter.ts`) ingests the
notification, self-maintains membership, and **synthesizes its own view frames** —
so #3 is pure duplication *when the entity store is active*.

**The gate (found in the map):** self-maintenance lives **only** in the
entity-store adapter. The mail-list hook (`useRuntimeMailListView`) *ignores*
`notification` frames — with the store **off** (`VITE_ENTITY_STORE=false`), #3 is
the **only** thing that updates the list. So removing #3 **requires** retiring the
`VITE_ENTITY_STORE` opt-out (committing to the entity store as the sole path).
That is the prerequisite decision, not a detail.

`event_affects_view` also serves `MessageDetail` / `Conversation` /
`AccountStatus` recomputes — the entity store does **not** self-maintain those, so
#3 must stay for them. Option iii neuters #3 **only for `ViewKind::MailList`**.

### Concrete migration (supersedes the sketch above)

1. **Retire `VITE_ENTITY_STORE`** (commit to the store) — **DONE.** The opt-out
   was removed; `installEntityStoreAdapter()` is unconditional, so no path
   depends on the runtime's per-event re-serve. (The base HTTP adapter remains
   only as the wrapped base + the WASM-load bootstrap window.)
2. **Confirm the entity store self-maintains everything #3 covers** — **DONE,
   and it found the real blocker:** the store dropped *projection-less* sync
   events (IMAP expunge / membership-removal / delete), for which #3 was the only
   corrector. Fixed in [[L2-projectionless-sync-events]] (fix a) — the three sync
   emitters now attach `projection` + `countDeltas`, so the store self-maintains
   rows + counts on those paths. This unblocks #3 removal.
3. **Neuter #3 for `ViewKind::MailList`** in `spawn_event_pump` (skip
   `send_recomputed_replace`); keep it for the other view kinds and keep
   #1/#2/#4/#5/#6 untouched.
4. **Harden gap-detection** (#4/#5/#6 become the *only* correctors) before, not
   after.
5. **Measure** the recompute drop.

## Provenance

Architectural discussion during the move/delete-flicker fix (2026-06-27), after
shipping the `set_view_rows` reconcile. The reconcile addressed the symptom; the
user flagged the dual-source path as a code smell and asked for a single
source of truth / communication channel between runtime and client.
