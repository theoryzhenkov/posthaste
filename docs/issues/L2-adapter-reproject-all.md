---
scope: L2
summary: "The entity-store adapter re-projects EVERY open view on EVERY drain (serialize+parse+full-string-compare across the WASM boundary), ignoring the store's precise dirty View keys — because a content-only rederive_message doesn't mark the view dirty. O(views × rows) per event; the O(all-messages) recompute pattern resurfaces during sync bursts."
modified: 2026-06-27
reviewed: 2026-06-27
lifecycle: ephemeral
type: ISSUE
status: open
priority: medium
depends:
  - path: docs/eph/DESIGN-L2-client-link-reactive-store
---

# Adapter re-projects all views every drain (perf)

`drainAndEmit` (`apps/web/src/runtime/replica/entityStoreAdapter.ts:415`) drains
the dirty keys but uses only the `mailbox` ones; `emitChangedViews`
(`:427`) then loops **all** views and calls `projectViewJson` for each — which
serializes every row *with its full message projection* across the WASM boundary
(`crates/posthaste-link-wasm/src/entity_store.rs:206`), JSON-parses it back, and
full-string-compares against `lastProjectionJson`. That is O(views × rows)
serialize+parse+compare per event; during a sync burst (many `message.updated`)
this is the O(all-messages) recompute pattern from the link-bus perf regression.
The JSON-diff gate prevents *emitting* unchanged views downstream but does not
avoid the per-drain projection cost.

**Root cause it's brute-forcing:** the store does **not** mark a view dirty on
content-only changes — `rederive_message`'s `(true, Some(idx))` arm only sets
`DirtyKey::View` when the row *tuple* (`rowKey/messageId/sortKey`) changes; a
flag/read toggle leaves `sort_key` unchanged → `changed=false` → no `View` dirty
key. So the adapter can't trust the dirty View set (its own comment admits this).

**Fix:** add a message→views reverse index in the store so a content-only
`rederive_message` marks the owning views dirty, then re-project only the views
named in the drained `View` keys. This removes both the all-views scan and the
missed-content-dirty gap — and the reverse index is also what
[[L2-reserve-clobbers-optimism]]'s reconcile-on-re-serve wants.

## Provenance

Four-reviewer Task 3 (HIGH-2).
