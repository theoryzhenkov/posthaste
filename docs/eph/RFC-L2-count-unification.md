# RFC-L2: Unify mailbox counts on invalidation (retire the countDelta channel)

> Post-beta cleanup. The mailbox unread/total count architecture has produced the
> same bug three times (§0 source-lag, projection-less-drop, split-drop). Retire
> the fragile `countDelta` propagation and put ALL counts on the one model that
> never broke: react-query invalidation → refetch the authoritative count.
> Grounded in the 2026-07-06 investigation. NOT beta-blocking — the delta path now
> works (all three bugs fixed); this is simplification, execute deliberately.

## Context — why counts are hard here

The client entity-store is a **partial window** (`entity_store.rs:42`: *"the store is partial, so a held-window count is not the true total; a count delta from the authority is the only path"*). So the client can't derive true counts from its held rows, and the authority owns them.

Today the authority ships counts as **`countDeltas` piggybacked on `message.updated`** across every topology:
- bundled echo (store command computes them in-tx, §0/`ff6b2d0d`),
- sync,
- split (assertion now carries the enriched event, this session's fix).

The client applies them incrementally to a live-store slice (`apps/web/src/live-store/store.ts`). **This is the fragility:** every emit path must attach deltas, the client must apply each exactly-once, and a single miss = wrong-until-reload. We've now patched three separate drop points.

**The tell:** smart-mailbox counts **never had any of these bugs** — they were *not* migrated (`M46/D116`) onto the delta slice. They use **react-query invalidation**: on a relevant change, invalidate → refetch the current authoritative count (the synchronously-written canonical row). Self-correcting, one mechanism, topology-agnostic, no deltas.

## Decision

**D1 — Put source-mailbox counts back on react-query invalidation (the smart-count model), and delete the `countDelta` live-store slice + all delta-attachment plumbing.** One count mechanism for both count types.
- The count becomes a react-query keyed per (account, mailbox) whose queryFn reads the authoritative count from the runtime (the canonical `unread_emails`/`total_emails` the store maintains via triggers).
- On any event that can change a count (`message.updated`, sync, settle), **invalidate** the affected mailbox count key → react-query refetches the current value. No delta computation, no exactly-once application, no reload-to-resync.
- **Server side:** stop computing + attaching `countDeltas` to `message.updated` (bundled command, sync, and the split assertion-carry) — the events shrink to their non-count payload (`projection` may stay for the row/view fold; counts leave). The store's count triggers remain the source of truth.
- **Client side:** delete the live-store count slice + `storeUpdatesFromEvent`'s countDelta routing + the `entityStoreAdapter` count plumbing. `useMailboxCounts` becomes a react-query hook (mirroring the smart-count hook).

**D2 — Optimism (optional, additive):** react-query refetch has a small round-trip vs the delta's instant local apply. For the user's OWN mutation, keep it feeling instant with a **local optimistic decrement** on the affected count key (react-query `setQueryData`), reconciled by the invalidation refetch. This is a thin, self-correcting overlay — not the fragile delta channel (a missed optimistic update just means the refetch lands a beat later, never a permanently-wrong count).

## Why this is cleaner
- **One mechanism** (invalidation) for smart + source counts — the model already proven bug-free.
- **Self-correcting**: a count is always the current authoritative value after a refetch; a missed invalidation is at worst a brief staleness the next event corrects — never wrong-until-manual-reload.
- **Topology-agnostic**: bundled and split both just invalidate + refetch (split refetches over the link). The split-carry fix (this session) becomes unnecessary and can be removed.
- **Deletes a subsystem**: the countDelta computation (server, 3 paths) + the live-store slice + the exactly-once application (client) all go away.

## Considered alternative (REJECTED — owner decision 2026-07-07)
**Derive counts locally from a complete client-side count-index** (`message_id →
{mailbox_ids, is_read}` for ALL messages; MessageSummary already carries both
fields). Instant + offline + optimism-for-free — but **in a bundled deployment it
duplicates data the runtime already holds and already maintains** (the complete
replica + trigger-maintained canonical counts, in-process, one indexed row read
away). Storing a second complete index on the client to re-derive what the
runtime knows is exactly the redundancy the layered-store design should NOT
extend: the client store earns its bundled-mode redundancy as a bounded *window*
(optimistic UI + one client for both deployments); a *complete* index crosses
that line. The bundled "round-trip" under D1 is an in-process loopback read of
one indexed row — effectively free — so local derivation buys nothing there and
costs a duplicated store everywhere. Revisit only if split-mode refetch latency
proves unacceptable in practice.

## Slices
- **C1** — `useMailboxCounts` → react-query (mirror the smart-count hook); dual-read alongside the slice behind a flag.
- **C2** — invalidation wiring: every count-affecting event invalidates the affected mailbox count key(s).
- **C3** — optimistic decrement overlay (D2) for the user's own mutation.
- **C4** — delete the countDelta slice + server-side countDelta computation (all 3 paths) + the split assertion-carry; remove the plumbing.

## Risks
- Refetch latency on a busy account (many invalidations → many refetches) — batch/debounce invalidations; the count read is a cheap canonical-row read.
- The optimistic overlay must reconcile cleanly (setQueryData then invalidate) — keep it thin.
- Split mode refetches over the link (a round-trip) — acceptable; correctness over the micro-latency.

## Out of scope
Message-list liveness (a separate, working system); the metadata-complete-store alternative; anything non-count.
</content>
