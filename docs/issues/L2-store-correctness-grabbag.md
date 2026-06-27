---
scope: L2
summary: "Assorted smaller correctness + robustness items in the reactive store: in_range ignores sort direction (Asc views place rows backwards); lexicographic received_at sort is fragile to mixed timestamp formats; unbounded base/message/row accumulation with no GC; writeMailboxCount silently drops counts for an unknown account; settleAll is fire-and-forget with unguarded async; plus low-severity nits."
modified: 2026-06-27
reviewed: 2026-06-27
lifecycle: ephemeral
type: ISSUE
status: open
priority: medium
depends:
  - path: docs/eph/DESIGN-L2-client-link-reactive-store
---

# Store correctness + robustness grab-bag

Independently small, kept together for "don't forget." Each is self-contained.

## A — `in_range` ignores sort direction; Asc views place rows backwards (HIGH)

`crates/posthaste-link-replica/src/entity_store.rs:535`. The doc promises
direction-dependent behavior (Desc: `sort_key >= W`; Asc: `sort_key <= W`) but
the function takes no direction and unconditionally returns `sort_key >= W`.
`insert_sorted` *does* branch on direction, so the two disagree for
`SortDirection::Asc`: an Asc view treats its watermark boundary inverted. Latent
(default Desc) but `Asc` is a plumbed, accepted value that silently corrupts
membership. **Fix:** pass `direction` into `in_range`, flip for `Asc`; add an
Asc-view placement test. (Task 1 H2.)

## B — Lexicographic `received_at` sort is format-fragile (MEDIUM)

`entity_store.rs:79` (`SortKey` derives `Ord`). Lexicographic == chronological
only for uniform zero-padded same-precision same-offset ISO-8601. Mixed
fractional precision (`…00Z` vs `…00.500Z`) or `+00:00` vs `Z` reorders. Depends
entirely on an undocumented, untested upstream format guarantee. **Fix:** assert/
normalize the timestamp format at ingest, or parse to a real instant for `Ord`;
add a mixed-precision test. (Task 1 M5.)

## C — Unbounded base/message/row accumulation, no GC (MEDIUM)

`entity_store.rs:251`, `364`. `apply_message` inserts into `messages` +
`engine.base` for *every* ingested message, including out-of-range ones placed in
no view; `insert_sorted` only ever grows `rows` (no window cap / `W`-tightening
eviction). A long-lived session over a busy account grows `messages`,
`engine.base`, and `view.rows` unbounded. Acknowledged in comments but neither
bounded nor flagged. **Fix:** don't retain base for messages matching no open
view; implement tail eviction with the watermark tightening. (Task 1 M4.)

## D — `writeMailboxCount` drops counts for an unknown account (MEDIUM)

`entityStoreAdapter.ts:472`. `mailboxAccount` is populated only from
`message.updated` events carrying both `accountId` and `countDeltas`; the
open/snapshot seeding uses `countDeltas:[]`. So a mailbox dirtied before any
account-bearing delta hits the early return and the count is dropped (and the
REST fallback is now skipped). Secondary: `setQueryData(old?.map(...))` no-ops if
the mailboxes query isn't cached yet. **Fix:** seed `mailboxAccount` from the
open request's scope at `openMailListView`; when the account is unknown, fall
back to a mailbox invalidation rather than dropping. (Task 3 MEDIUM-4.)

## E — `settleAll`/`onBaseFrame` are unguarded async (MEDIUM)

`entityStoreAdapter.ts:359` (`void this.settleAll(...)`) and the `runMutation`
catch ignore rejections from `outbox.remove`/`drainAndEmit` → unhandled promise
rejections; the `settle` return `reverted` flag is discarded. The per-frame store
calls in `onBaseFrame` aren't wrapped — a single throw out of `onFrame` can tear
down the SSE subscription pipeline. **Fix:** try/catch with a logged failure in
`settleAll`; guard the `onBaseFrame` store calls. (Task 3 MEDIUM-6.)

## Low-severity nits

- `expect()`s in `rederive_message` (`entity_store.rs:419,429,438`) are safe but
  fragile to refactors — prefer `if let`/`continue`. (Task 1 L7.)
- `set_view_rows` no-ops on an unregistered view (masks host bugs); `close_view`
  leaves a stale `DirtyKey::View` in the dirty set. (Task 1 L8.)
- `CountDelta` is an absolute snapshot, not a delta, despite the name — rename or
  document to stop a future additive misuse. (Task 1 L9.)
- Synthesized `viewReplace.sessionSeq` comes from a private `1_000_000+` counter
  disjoint from real base seqs — poisons any future resume-by-`afterSeq`/dedup.
  (Task 3 LOW-7.)
- `projectView` fabricates `orderKey: ''` and drops `sortKey/pendingMarkers`;
  `ingestMessageEvent` reads `inner.messageId` from the payload instead of the
  canonical top-level `event.messageId`; `linkRuntimeMutationId` stores a
  durable id that's never read (dead metadata). (Task 3 LOW-9.)

## Provenance

Four-reviewer Tasks 1 + 3 (the assorted HIGH/MEDIUM/LOW items not bundled
elsewhere).
