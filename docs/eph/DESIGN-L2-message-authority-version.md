---
scope: L2
summary: "Per-message authority-state version on MessageSummary — the staleness guard that stops a late provider re-serve from clobbering a confirmed optimistic mutation (flicker Bug 1b)"
modified: 2026-06-27
reviewed: 2026-06-27
lifecycle: ephemeral
type: DESIGN
depends:
  - path: docs/eph/DESIGN-L2-mutation-notification
  - path: docs/eph/DESIGN-L2-client-link-reactive-store
dependents: []
---

# Per-message authority version (flicker Bug 1b)

## Problem

The nightly mail-list flicker was diagnosed to two coupled bugs in the client
replica (see the `flicker-root-cause` memory). Bug 2 (absorption order) and
Bug 1a (retire on an unconfirmed local echo) are fixed in `link-core`/`link-replica`
(confirmed-gated retire). The residual tail — **Bug 1b** — is: after an
optimistic op is authority-confirmed and retired, a *late* provider-sync
`message.updated` whose snapshot predates the mutation re-serves the old
projection and clobbers the confirmed base (no pending op left to fold it back)
→ the row reverts → flicker until the next sync corrects it.

`ingest_batch` trusts every base update; the served `MessageSummary` carries no
per-message version, so a stale re-serve is byte-indistinguishable from a
legitimate change. The guard needs a version.

## The version must be provider-causality-ordered

Ruled out (each lets the stale re-serve win):
- **stream / firehose seq** — a late re-serve has a *higher* stream seq.
- **store commit counter / `updated_at`** — the stale sync commits *later*, so it
  bumps the counter higher despite older content.

The version must reflect the **provider state the data came from**. After a
confirmed mutation the provider has advanced its per-object version; a stale
re-serve carries the older one. (During the optimistic window the provider has
*not* advanced yet — but there Bug 1a's confirmed-gating keeps the op folded, so
1b only has to cover the post-confirm tail, where the provider version is real.)

## Source per provider

| Driver | Source | Notes |
|---|---|---|
| IMAP | per-message `max(modseq)` across the message's `imap_message_location` rows | Already stored; clean and monotonic per RFC 7162. A confirmed STORE bumps modseq, the next FETCH carries it. |
| JMAP | **none today** | JMAP has no per-object version (changes are account-level state + `/changes`). Deferred — see Open questions. |
| Mock / local | none | No concurrent provider re-serve, so no staleness race. |

## Shape + contract

- `MessageSummary` gains **`version: Option<u64>`** (camelCase `version` on the
  wire). Opaque monotonic per-message authority version; higher = newer.
- **Absent ⇒ unguarded.** The replica guard engages only when *both* the
  incoming and held bases carry a version; otherwise it accepts (no-op). This
  makes the field additive/backward-compatible and lets JMAP/mock ship unchanged,
  relying on Bug 1a's confirmed-gating for the common case.
- **Guard (replica, `apply_message`):** reject an incoming base iff
  `incoming.version < held.version` (strict). Equal = idempotent (accept/no-op).

Sample (IMAP message):

```json
{ "id": "…", "sourceId": "…", "receivedAt": "…", "mailboxIds": ["…"],
  "keywords": ["$flagged","$seen"], "isRead": true, "isFlagged": true,
  "version": 4291 }
```

## Division of work

- **link-core / link-replica (views-stability):** Bug 1a confirmed-gated retire
  (✅ landed); the `apply_message` version guard tolerant of a missing `version`
  (✅ landed; strict `<`, equal = accept, either-absent = unguarded).
- **runtime / store (this line, ✅ landed):** `version: Option<u64>` on
  `MessageSummary`, sourced from IMAP `max(modseq)` (`CAST AS INTEGER`) via
  `fetch_message_version_tx`, wired into both the list hydrate
  (`query/summaries.rs`) and the canonical detail/`message.updated` builders
  (`query/details.rs`). JMAP/mock leave it `None`. openapi.json +
  web/mcp `schema.gen.ts` regenerated.
- **Shared red→green target:** the `BUG-1 guard target` case in
  `apps/web/test/replicaAbsorptionRetire.test.ts` (real-WASM): seed@v1 →
  optimistic flag → confirm flagged@v2 → stale re-serve@v1 must be rejected.
  Green on the views-stability line (guard); goes green end-to-end on merge.
- **Real-data proof (this line):** `gmail_inbox_sync.rs` asserts a live IMAP
  sync stamps `version = max(modseq)` (=100) into the view-row projection
  end-to-end (sync → store → projection → frame). Store unit:
  `tags_and_locations.rs::message_summary_carries_max_modseq_as_version`.

## Open questions

1. **JMAP per-message version.** No per-object version exists. Options: derive a
   per-message version from the `/changes` state seq at last fetch (store-side,
   monotonic per object), or accept JMAP stays unguarded (1a-only) until a
   dedicated design. Recommend: ship IMAP-guarded first, JMAP unguarded, file a
   follow-up. The user's flicker is on a Gmail-over-IMAP account, so IMAP-first
   closes the reported case.
2. **Optimistic base events.** If the runtime emits a *base* `message.updated`
   for *local* optimism (it does in-process), it carries the pre-mutation
   version — harmless under 1a (op folds; the echo never retires) and under the
   strict-`<` guard (equal/older, no clobber of a newer confirmed base). Confirm
   the web runtime path agrees.
3. **Where to stamp.** Cleanest at the read-model/summary builder
   (`posthaste-store/src/query/summaries.rs` + the IMAP location join) so both
   the view snapshot rows and the `message.updated` projection carry it from one
   source.
