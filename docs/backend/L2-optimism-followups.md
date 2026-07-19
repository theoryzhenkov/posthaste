---
title: "L2-optimism follow-up ledger"
scope: tracking
summary: "Debt and deferred items surfaced by the adversarial review + gate run of the atomic/pure replay refactor. Not a spec — a tracked backlog so it does not dissolve into folklore."
state: active
modified: 2026-07-18
reviewed: 2026-07-18
depends:
  - path: docs/backend/L2-optimism
  - path: docs/backend/L1-backend
---

# L2-optimism follow-up ledger

The refactor that made the replay engine atomic and pure landed green (unit,
integration, backend lib, send-path gate, live Stalwart 4/4, `fmt --check`,
`clippy -D warnings`). The review found the architecture sound and the four
contract slices intact; the items below are debt it surfaced — tracked here,
prioritized, with locations and fix sketches.

## P1 — phantom-prevention core

These two sit inside the "words are never silently dropped" guarantee and are
the highest priority.

### F6. `OUTBOX_LIST_SAFETY_LIMIT = 5000` is baked into the atomicity core

`list_unsettled_operations` / the derive's in-txn log read cap the fold at
5000 ops (`crates/posthaste-store/src/outbox.rs:28`, used at `outbox.rs:317`,
`overlay.rs:130`, `overlay.rs:192`). The const's own comment says "picked
sane, not measured." The fold now runs inside the atomicity core, so an
account that exceeds 5000 unsettled ops silently drops the **newest** ops
from the fold — a user's latest draft save vanishing from the view is exactly
the phantom-shaped failure the refactor eradicated elsewhere. Self-heals on
truncation (ops leave the log) but the window is real and wrong-shaped (drops
newest, not oldest).

**Fix:** page the fold, or make the limit an explicit part of the invariant
(and log + refuse when hit) rather than a silent truncation. Measure the real
ceiling before picking a number.

### F7. The content-kind list `('draftCreate', 'draftUpdate', 'send')` is duplicated across five sites

The "failed content ops stay foldable" SQL fragment is copy-pasted at
`outbox.rs:317`, `overlay.rs:130`, `overlay.rs:192`, plus the migration at
`schema.rs:514`/`661`, and must agree with the Rust predicates
`is_replayable`/`is_content_op` (`replay.rs`). Adding a new content-op kind
(a future draft shape) requires changing all of them in lockstep — a
miss-site silently breaks the "parked content stays visible" rule.

**Fix:** one source of truth — a `is_content_op_kind(kind)` used to build
the SQL fragment (or a stored set), and the Rust predicates calling the same
function. The three fold-SQL sites should share a single const/fragment.

## P2 — maintainability / dead surface

### F8. The apply-mutation match block is duplicated 4×

`match mutation { Upsert(record) => upsert_overlay_tx(…), Tombstone => …,
Remove => …, Keep => {} }` plus the `now_visible` derivation appears in both
`derive_overlay` (`overlay.rs:153-163`) and `remove_op_and_derive`
(`overlay.rs:213-223`), and twice more in the test store
(`store_sync_cache_impls.rs`). A new `OverlayMutation` variant must be
added at every site.

**Fix:** extract `apply_overlay_mutation_tx(tx, account_id, row_id, mutation)
-> bool` (returns `now_visible`) shared by both derives; the test store gets
a matching helper.

### F9. `replay_account_overrides` has no production caller

The full-rebuild recovery path (`replay.rs`) is referenced only by its own
doc comment and the tests. It is the documented recovery path for a wiped
derived view, but nothing wires it to a quarantine/rebuild trigger. It may be
dead, or it may be load-bearing for a path not yet exercised in production.

**Fix:** either wire it to the quarantine-and-rebuild path the model
specifies (L2-optimism "Durability classes"), or delete it and document that
the per-row `replay_base_write` + sweep is the only recovery. Do not leave it
ambiguous.

### F10. Three "under the old…" design-history comments

`tests/replay.rs:1415` and `:1505` (and one more) explain the new behavior by
contrast with the retired non-atomic / clock-based fold. The house rule is
that comments describe the current design, not its history — these age into
folklore and mislead once the "old" code is forgotten.

**Fix:** rewrite them to state the invariant directly ("the fold is a pure
function of (log, base); a Pending send with an elapsed but undispatched hold
stays held…") without the comparative frame.

## P3 — echo / TOCTOU (self-healing, plumbing exists)

### F4–F5. Two subtle echo/TOCTOU deltas in the truncate/adopt rewrite

The truncate/adopt echo now uses `DeriveDiff::retired()` (overlay-only), not
the previous effective-visibility (`get_message_summary` before/after). For
content-op rows (no base) the two agree; for base-backed rows they can differ
— a content-op row whose base exists should NOT emit `deleted` when its
overlay retires (base shows through), and `retired()` alone would. Audit
found two sites where this delta is observable.

**Fix:** extend `DeriveDiff` with `base_present` and an
`effectively_retired()` = `(base || was) && !(base || now)`; use it for the
echo. Computed in-txn, so it also closes the TOCTOU. (This is the same
plumbing the deferred item below needs.)

### Deferred. `unwind_send_fold` / `cancel` / `discard` echo sites still use before/after `get_message_summary`

These three (`outbox/queue.rs`, `outbox/draft.rs`) were not converted to
`DeriveDiff` in the refactor — they read the effective view before and after
a refresh, a TOCTOU under concurrency (self-healing on the next replay, but
real). The plumbing exists: `refresh_message_overlay` returns `DeriveDiff`.

**Fix:** convert the three sites to `refresh_message_overlay(...).await?.retired()`
(or `effectively_retired()` once F4–F5 lands), deleting the duplicated
before/after reads. This is the last echo-cleanup site.
