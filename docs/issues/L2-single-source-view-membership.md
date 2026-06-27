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
| Store-off fallback (`VITE_ENTITY_STORE=false`) | **keep** | re-serve is the only source then |
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

## Provenance

Architectural discussion during the move/delete-flicker fix (2026-06-27), after
shipping the `set_view_rows` reconcile. The reconcile addressed the symptom; the
user flagged the dual-source path as a code smell and asked for a single
source of truth / communication channel between runtime and client.
