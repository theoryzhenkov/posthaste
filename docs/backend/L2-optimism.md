# L2 — Optimism: the replay model

state: proposed, for redline
foundation: server reconciliation — Bayou's lineage; in production: Replicache,
Linear, Superhuman

## The model

Visible mail state is a function, not a place:

```
visible = replay(log, base)
```

`base` is provider truth, written only by sync. The `log` is the durable record
of un-acknowledged local intent. The derived view is the cached output of
`replay`, materialized for SQL and wipeable at any moment. Optimism, offline,
and recovery are the same operation: recompute the function when an input
changes.

## Three planes

**Base** — provider truth. Sync writes it; nothing else does. Re-syncable.

**The op log** — the outbox, read two ways. The flusher reads it as a queue:
deliver each op to the provider, in order, idempotently. Replay reads it as a
log: fold each op's effect over base. One table, one lifecycle, two readers;
there is no second structure.

**The derived view** — `replay(log, base)`, materialized into the tables the
query layer serves. It owns nothing and can always be rebuilt from its inputs.

## Ops

An op is a typed intent with a client-minted idempotent id. Two shapes, one
table:

**Intent ops** mutate existing messages: flags, moves, deletes, snooze. Their
replay effect folds over a base row. They are speculation: a permanently
rejected intent op dies visibly and base wins.

**Content ops** carry authored mail. `putDraft` holds full draft content,
last-write-wins by a stable draft id — successive saves supersede by id, a
superseded unflushed put is pruned, and identity never rotates. `send` holds
the full message and waits out the undo window in the log. Their replay effect
adds a row the provider does not have yet. They are never speculation: a
permanently failed content op stays parked in the outbox with its content
intact. Authored words are never silently dropped.

Three rules govern the log:

**Order.** The log is ordered. Replay folds ops in log order, and the flusher
delivers in the same order, so the provider observes the sequence the user saw.

**Determinism.** Replay is pure: every id, timestamp, and value an op needs is
captured at command time and carried in the op. Replay generates nothing, so
replaying twice yields the same rows.

**No wedging.** A permanently failed op leaves the flush lane immediately —
parked (content) or dead (intent) — and never blocks the ops behind it.

## Settlement and truncation

An op settles when the provider accepts it. Settlement records the provider's
outcome; for content ops that includes the provider-assigned id, which bridges
the local identity to the base identity until truncation.

Truncation is causal, not semantic. Where the provider names sync positions,
settlement captures the one that includes the change — a JMAP `set` returns
`newState` — and the op leaves the log once sync's state chain passes it.
Where the provider does not (IMAP), the op leaves once a sync cycle that
started after settlement completes. Both are ordering checks; no comparison
of state decides anything.

If a provider consistency blip ever breaks the causal assumption, the failure
is a one-cycle flicker corrected by the next replay — never durable wrong
state.

## Replay

Runs whenever an input changes: a log write (command accepted, op settled,
op truncated) or a base write (sync applied). It recomputes only the rows the
changed ops touch. A full rebuild is always legal and is the recovery path.

## Durability classes

The log is precious: it holds unacknowledged intent and authored content.
Base is re-syncable. The derived view is disposable. Quarantine-and-rebuild
honors these classes — it salvages the log, re-syncs base, and regenerates
the view.

## What this deletes

Pins and their naming conventions. The coverage check. The post-sync sweep.
Discard unwind bookkeeping — discarding is deleting the op; the next replay
forgets its effect. The orphan class loses its representation: the derived
view owns nothing, so nothing in it can outlive an owner.

## Migration — schema v5

One pass at upgrade, audited, never recurring:

- An overlay row with a live owning op is dropped; the op replays over base
  after upgrade.
- A pinned row carrying content that exists nowhere else becomes a parked
  content op — visible in the outbox for the user to keep or discard.
- Everything else is provably derived debris and is deleted.

## Execution

1. The replay engine and derived-view rebuild, behind the existing query
   layer — no wire change.
2. Causal truncation; the coverage sweep deleted.
3. Content ops (`putDraft`, `send`) with the identity bridge; pins deleted.
4. Quarantine salvage and the v5 migration.
5. The stranded-phantom reproduction test flips from pinning the bug to
   guarding its absence.

Every slice lands green through the send-path gate and the live Stalwart
suite.

## Prior art

Three production systems converged on this shape independently. Superhuman's
modifier queues: pure pending mutations recombined with the server cache per
thread; removal re-derives the view. Replicache: speculative mutations rebased
over each pulled snapshot, acknowledged by `lastMutationID` — causal, never
semantic. Linear: a persisted transaction queue rebased over sync-action
deltas, completed by a `lastSyncId` watermark; clients mint permanent ids so
nothing needs renumbering. The JMAP client guide teaches the same
speculate-then-reconcile pattern with creation ids.

The alternative — optimistic writes into the synced store itself, guarded by
dirty-markers and reconciliation (Mailspring, Thunderbird) — publicly exhibits
this system's retired bug class: archived mail resurrecting after sync, silent
divergence, one wedged op blocking the queue.
