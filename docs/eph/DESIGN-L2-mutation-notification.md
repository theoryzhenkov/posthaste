---
scope: L2
summary: "Replace the RuntimeFrame::MutationSettlement frame with a general mutation.notification envelope (mutationId + a polymorphic notification body: confirmed / rejected{error}), and change the client's confirmed semantics from retire-and-rederive to drop-when-the-base-absorbs-it — eliminating the settlement-vs-base-update frame race that flickers optimistic rows (most visibly on undo)."
modified: 2026-06-27
reviewed: 2026-06-27
lifecycle: ephemeral
type: DESIGN
depends:
  - path: docs/eph/DESIGN-L2-client-link-reactive-store
  - path: docs/eph/DESIGN-L2-optimistic-projection
  - path: docs/replication/client-link/L2
  - path: docs/runtime/mutations/L1
dependents: []
---

# A general mutation.notification, and a race-free retire

## 1. The problem: settlement races the base update

An optimistic row flickers when a mutation settles — most visibly on undo, but
the mechanism is general (flag/read flicker too). The renderer shows the
optimistic state, it briefly reverts to the pre-mutation state, then snaps to
the confirmed state.

The cause is two unordered frames for one logical event. When a message mutation
confirms, the runtime delivers the outcome over **two decoupled paths into the
session stream**:

- **The settlement frame takes the direct path.** `run_message_mutation` calls
  `settle_mutation(Confirmed)` synchronously after `forward.await`, which sends
  `RuntimeFrame::MutationSettlement` straight onto `session.frames`
  (`build.rs`, `sessions.rs`).
- **The authoritative base update (`message.updated`) takes the indirect
  path.** The backend publishes it onto the event bus *during* `forward.await`,
  but it reaches `session.frames` only after the separate, laggable
  `spawn_notification_forwarder` task drains the broadcast channel
  (`read.rs::run_backend_down_channel` → `down_assertion_to_event`). It carries
  a local monotonic seq and **no mutation id** — it is decoupled from the
  command that caused it.

The direct send almost always wins, so the client receives
`MutationSettlement(Confirmed)` *before* `message.updated`. Today
`EntityStore::settle(Confirmed)` immediately **retires the pending op and
re-derives**, so for that one frame the projection reverts to the stale base
(which does not yet carry the effect) — the flicker — until `message.updated`
lands and re-folds.

The spec's "convergence interval is invisible" guarantee (idempotent fold) only
covers the window where the base is updated *and the op is still pending*. The
bug is that the op is retired *before* the base updates, opening the one
unprotected window: `(stale base, no pending)`.

## 2. The fix in one sentence

**Optimism is retired only when the authoritative base demonstrably carries its
effect — never on a bare verdict that can outrun the base.**

A confirmed verdict stops *waiting*; it does not *revert*. The base update is
what retires the op (the op becomes redundant once the base absorbs it). This
makes the two frames commute: whichever arrives first, there is no revert
window.

## 3. The contract change

Replace the fixed-shape settlement frame:

```rust
// removed
RuntimeFrame::MutationSettlement { session_seq, mutation_id, state: RuntimeMutationSettlement }
```

with a general envelope keyed by the client mutation id and carrying a
polymorphic notification body:

```rust
RuntimeFrame::MutationNotification {
    session_seq: RuntimeSessionSeq,
    mutation_id: ClientMutationId,
    notification: MutationNotification,
}

/// A terminal (or future non-terminal) signal about a named mutation, keyed to
/// the client mutation id so the client can correlate it to its outbox op.
#[serde(tag = "type", rename_all = "camelCase")]
pub enum MutationNotification {
    /// The mutation succeeded. The authoritative base carrying its effect
    /// arrives (or has arrived) via `message.updated`; this is the verdict, not
    /// the data. The client retires the op once the base absorbs it (or
    /// immediately, if the effect was a no-op the base already reflects) — it
    /// never reverts on this signal.
    Confirmed,
    /// The mutation was rejected. No base update will carry its effect, so the
    /// client drops the op, reverts the optimistic projection, and surfaces the
    /// error. `Conflict` collapses into this — the distinction lives in the
    /// error code.
    Rejected { error: RuntimeAdapterError },
}
```

Notes:

- **Keyed by `ClientMutationId`, not `RuntimeMutationId`.** The client's outbox
  is keyed by the client mutation id; the notification must correlate to that.
- **`RuntimeAdapterError` is reused** for the rejected body — it already carries
  `code` / `message` / `retryable` / `correlationId` / `details`.
- **`Failed` + `Conflict` collapse into `Rejected`.** The 6-state
  `MutationSettlementState` (Accepted / LocalApplied / Queued / Confirmed /
  Failed / Conflict) shrinks on the wire to two terminal outcomes. The
  non-terminal acks (Accepted / LocalApplied / Queued) are **dropped from the
  client stream** — the client already tracks the op locally the moment it
  enqueues it, and the synchronous `MutationReceipt` still carries state for the
  caller. (`MutationSettlementState` stays in `MutationReceipt` for now;
  trimming it is a follow-up.)
- **`message.updated` is unchanged** — no mutation-id tagging, no merge. The
  base-update path stays exactly as it is; only the *verdict* frame changes.

## 4. The client semantics (the actual race fix)

The lifecycle of one optimistic op, after this change:

| Event | Store action |
| --- | --- |
| dispatch | fold the assertion into the outbox (optimistic) |
| `message.updated` ingest | update the base; **drop any pending op the new base now absorbs** (`apply(newBase, op) == newBase`) |
| `mutation.notification {confirmed}` | drop the op **iff** the current base already absorbs it; otherwise leave it folded (idempotent → invisible) for a later base ingest to drop. **Never rederive-to-revert.** |
| `mutation.notification {rejected}` | drop the op (if still pending) + rederive (revert) + surface the error |

Why each case is flicker-free:

- **State-changing success, settlement first:** `confirmed` arrives, base not yet
  absorbed → op left folded (no revert). `message.updated` arrives → base
  absorbs → op dropped. No revert window.
- **State-changing success, base first:** `message.updated` absorbs the op → op
  dropped. `confirmed` arrives → op already gone → no-op.
- **No-op success** (e.g. flag an already-flagged message → no `message.updated`
  ever): `confirmed` arrives → the op is already absorbed by the base (applying
  it is a no-op by definition) → dropped immediately. No leak.
- **Rejection** (no base update): base never absorbs → op stays folded →
  `rejected` drops + reverts. Correct.
- **Rejection after coincidental absorption** (another actor produced the same
  state, then our mutation fails): `message.updated` from the other actor
  absorbs our op → dropped. A later `rejected` finds nothing to revert → no-op,
  which is *correct*: the authoritative base reflects reality; we must not
  revert a state another actor legitimately set.

### Engine change (`posthaste-link-core`)

`MessageReplica` / `Replica<C>` gains absorption-aware retirement:

- `set_base(key, base)` — after updating the base, drop pending ops on `key`
  that the new base absorbs (`replay(base, [op]) == Present(base)` for the
  message fold).
- `settle(id, Confirmed)` — drop if the current base absorbs the op; else a
  no-op (leave it pending; a later `set_base` GCs it). Distinct from today's
  unconditional remove.
- `settle(id, Rejected)` — remove (as today's `Failed`).

The absorption test reuses the existing idempotent fold: an op is absorbed when
folding it over the base produces the base unchanged. This is the same
mechanism that already makes the convergence interval invisible — now also the
retirement trigger.

## 5. Runtime emission change

`run_message_mutation` (`build.rs`):

- **Success** → emit `MutationNotification { mutation_id, Confirmed }` instead of
  `settle_mutation(Confirmed)`. The base update still flows via the firehose
  (unchanged). The direct-send `Confirmed` no longer races anything because the
  client no longer reverts on it.
- **Error** → emit `MutationNotification { mutation_id, Rejected { error } }`
  instead of `settle_mutation(Failed/Conflict)`.
- The non-terminal `Accepted` send in `accept_mutation` is dropped.

`sessions.rs` keeps the direct `session.frames.send` for notifications (they are
reliable session state, not firehose facts) and the reconnect re-emission of
recent terminal notifications (replacing the settled-mutation replay).

## 6. Scope and slices

1. **Contract** (`posthaste-runtime-contract`): the new frame variant + enum;
   keep `MutationReceipt` as is.
2. **Engine** (`posthaste-link-core`): absorption-aware `set_base` + `settle`,
   test-first.
3. **Store + WASM** (`posthaste-link-replica`, `posthaste-link-wasm`): thread the
   confirmed/rejected semantics through `EntityStore::settle`; the settle JSON
   boundary distinguishes the two outcomes.
4. **Runtime** (`posthaste-runtime`): emit the new frame; drop `Accepted`;
   reconnect re-emission.
5. **Client** (`apps/web`): handle `mutation.notification` in the entity-store
   adapter (`settleAll` → confirmed/rejected); surface the rejection error;
   delete the `MutationSettlement` handling.

Non-goals: tagging `message.updated` with a mutation id (not needed — absorption
correlates); count optimism (still authority-only); trimming
`MutationSettlementState` from the receipt.
```

