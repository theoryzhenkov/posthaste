---
scope: L2
summary: "The race-free retire guarantee lives only in EntityStore::settle; the generic convergence engine doesn't enforce it. Base-update methods (set_base/apply_base_update/replace_base) don't retire absorbed ops, and Replica::settle(Confirmed) still does the pre-fix unconditional retire — so a future caller using those directly silently reopens the flicker, with lower-layer tests still passing."
modified: 2026-06-27
reviewed: 2026-06-27
lifecycle: ephemeral
type: ISSUE
status: open
priority: low
depends:
  - path: docs/eph/DESIGN-L2-mutation-notification
---

# Engine-layer absorption footguns

**Status: PARTIALLY RESOLVED (downgraded to LOW), 2026-06-27.** The dead
base-update methods the original finding named are gone (`apply_base_update` /
`replace_base` / `project_all` were deleted in the legacy-cleanup arc), and
`Replica::settle(Confirmed)` is no longer the pre-fix *unconditional* retire — it
is now confirmed-gated (the engine owns a `confirmed` set; an op retires only
once confirmed AND absorbed). The engine also gained `mark_confirmed` +
`retire_absorbed_if` for the caller-gated path.

**What remains (the residual LOW footgun):** the *version* gate (hold an op until
a strictly-higher base version absorbs it — the equal-version fix) lives in
`EntityStore` (link-replica), not the generic engine. A future caller using the
engine's `settle`/`retire_absorbed` directly gets confirmed-gating but NOT
version-gating, so an equal-version stale re-serve could re-open the move flicker
for that caller. Today the only caller is `EntityStore`, which is correct. If a
second convergence consumer appears, lift the version notion into the engine
(e.g. `Convergence::version_of` with a `None` default) so the invariant is
enforced in one place. Until then, this is documentation, not an active bug.

---

**Original finding (preserved).**

The mutation.notification design (§4) specifies that a base update *itself* drops
absorbed ops and that confirmation never reverts. But the guarantee is enforced
only in the **store** layer; the generic `Replica<C>` engine still exposes the
pre-fix shapes, so the invariant is one careless caller away from breaking.

## A — Absorption is decoupled from base updates (MEDIUM)

`Replica::set_base` (`crates/posthaste-link-core/src/convergence.rs:130`) just
inserts; `apply_base_update` explicitly does **not** retire
(`convergence.rs:156` + comment); `replace_base` retires nothing. The store
compensates by manually calling `retire_absorbed` right after `set_base`
(`crates/posthaste-link-replica/src/entity_store.rs:371`). But `apply_base_update`
is the named "authoritative base update" entry point and retires nothing — a
future caller wiring it gets stale pending and a **reopened flicker window**, the
exact bug this work closed. This is the one genuine leaky-abstraction seam in the
otherwise-clean generalization.

**Fix:** fold absorption into `set_base`/`apply_base_update`/`replace_base`
(consistent with the design), or rename them to advertise that absorption is a
separate required step and document the pairing at the type level. At minimum
make `apply_base_update` retire absorbed ops so both base-update paths behave
identically.

Provenance: four-reviewer Task 1 (M3).

## B — `Replica::settle(Confirmed)` still does unconditional retire (MEDIUM)

`Replica::settle` retires the op by id for **both** outcomes
(`convergence.rs:191`, `retain(|held| &held.id != id)`), and its test
`confirmation_retires_pending_and_base_carries_the_effect` + the
`SettlementOutcome::Confirmed` doc-comment still bless the *pre-fix*
unconditional-retire model. The flicker fix lives only in `entity_store::settle`,
which routes `Confirmed` → `retire_absorbed` and never calls `engine.settle` for
confirmations. A future caller using `Replica::settle(…, Confirmed)` directly
**reintroduces the flicker**, and the lower-layer tests still pass.

**Fix:** remove/guard the `Confirmed` arm of `Replica::settle` (make confirm
absorption-only at the engine layer too), or at minimum add a doc/test note that
`Confirmed` must go through `retire_absorbed`; align the convergence test +
comment with the fix.

Provenance: four-reviewer Task 4 (M1).

## C — `retire_absorbed` running state diverges from fold after `Removed` (LOW)

`convergence.rs:232`: a `Removed` op is kept but `running` is not advanced to
"removed," so a subsequent op on the same key is judged against a still-present
state the real fold (`replay_message`, where Destroy is terminal) would never
reach. Can only over-keep an op (minor leak) or harmlessly drop a no-op-in-
reality op — never projection corruption — and ops-after-destroy shouldn't occur
(coalesce collapses to `Destroy`). Worth a comment or an explicit "once Removed,
stop folding and keep the rest."

Provenance: four-reviewer Task 1 (L6).
