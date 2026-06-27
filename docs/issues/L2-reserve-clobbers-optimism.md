---
scope: L2
summary: "The runtime view re-serve path (viewSnapshot/viewReplace) is trusted blindly by the entity store: it overwrites optimistic membership and can clobber confirmed content with stale authoritative rows, producing the on-mutation flicker (rows, and flag/read on undo) — especially during background sync."
modified: 2026-06-27
reviewed: 2026-06-27
lifecycle: ephemeral
type: ISSUE
status: open
priority: high
depends:
  - path: docs/eph/DESIGN-L2-client-link-reactive-store
  - path: docs/eph/DESIGN-L2-mutation-notification
---

# Re-serve clobbers optimism (the on-mutation flicker)

**Status: OPEN — user-reported, partially confirmed in code.** Found in the
flicker-investigation round (2026-06-27), *not* by the four-reviewer pass (which
was scoped to the mutation/settlement path and never traced the re-serve path).

The reactive store's flicker fix made *settlement* race-free (`retire_absorbed`),
but the **re-serve path** — the reused runtime view-serving structure
(`viewSnapshot`/`viewReplace` → `set_view_rows`) — is still trusted
**unconditionally**. It neither re-applies the store's optimism nor checks
freshness against what the store already knows. Background sync fires this path
frequently, so the flicker is "especially during sync."

## A — Re-serve overwrites optimistic membership (confirmed)

`EntityStore::set_view_rows` (`crates/posthaste-link-replica/src/entity_store.rs:254`)
blindly replaces `view.rows` with the served authoritative rows and re-applies
no pending optimism. The adapter calls it on every re-serve
(`apps/web/src/runtime/replica/entityStoreAdapter.ts:307`): `ingestBatch` (which
*does* correctly re-derive optimistic membership) is immediately followed by
`setViewRows`, which **overwrites that re-derived membership** with the served
set.

**Effect:** a message optimistically moved/deleted (folded `replaceMailboxes` /
`destroy`) is dropped from the view by `rederive_message`, then a sync re-serve
puts it back (the runtime's authoritative rows still contain it, since the
client's optimism is unknown to the runtime). On the next event it drops again →
**row flickers out → in → out**, repeatedly under sync. (Note: archive/trash/
moveToRole are *not* folded — they round-trip — so they don't exhibit this; the
foldable membership ops `moveToMailbox`/`replaceMailboxes`/`destroy` do.)

**Fix:** make `set_view_rows` reconcile, not clobber — after adopting served
rows, re-apply pending optimism (iterate `engine.pending()` message ids and
`rederive_message` each). The store already has both primitives; they just
aren't invoked on the re-serve path. Add a regression test: "re-serve preserves
an optimistic move/destroy."

## B — Stale/authoritative re-serve clobbers confirmed content (hypothesis)

Leading explanation for the **undo-only flag/read flicker**. The adapter's
`viewSnapshot`/`viewReplace` handler has **no revision/staleness guard**
(`entityStoreAdapter.ts:307–326`): it does `entry.lastSnapshot = frame.snapshot`
and `setViewRows(servedRows)` unconditionally. Once an optimistic op has retired
(absorbed on confirm, so no pending op remains to re-fold), a re-serve that
reflects the *pre-mutation* state overwrites the confirmed state → the flag/read
flashes back, then corrects.

Why undo makes it glaring: undo fires immediately after an action (max chance of
an in-flight, slightly-stale re-serve landing in the retire window) and is the
one operation where the row is expected to look *exactly* as before, so any
flash is obvious. Normal `setKeywords` has the same structural exposure but is
less noticed.

**Open question to disambiguate (asked, awaiting answer):** does the undo
flag/read flicker happen *without* background sync, or only with it? Only-with-
sync ⇒ confirms this stale-re-serve mechanism; even-without ⇒ a settle-timing /
second-path remnant specific to `applyDiff` (would need frame logging to catch).

**Fix:** a stale-re-serve guard in the adapter — ignore or merge a re-serve
whose `revision` is behind the latest the store has applied, rather than
clobbering. Pairs with (A): "reconcile, don't clobber."

## Related (lower)

- `viewSnapshot` is swallowed and re-emitted as `viewReplace`
  (`entityStoreAdapter.ts`, `emitChangedViews` always emits `viewReplace`) —
  loses the snapshot-vs-replace distinction if the renderer ever diverges
  (scroll/selection reset on a full snapshot). (Four-reviewer Task 3, LOW-8.)

## Provenance

Flicker-investigation round, 2026-06-27. (A) confirmed in code; (B) strong
hypothesis pending the with/without-sync answer. Both share one root: the reused
runtime view-serve path is trusted blindly by the store.
