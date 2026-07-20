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

**Progress (2026-07-18):** ALL ITEMS LANDED. F7/F8/F10 (`dd1589b9`),
F4–F5+F11 (`b83ba529`), F9 (`f8dd8ba0`), F6 (this change). The ledger is now
history — kept so the original findings stay traceable.

## P1 — phantom-prevention core

These two sit inside the "words are never silently dropped" guarantee and are
the highest priority.

### F6. ✅ LANDED — the 5000-op cap is removed

`OUTBOX_LIST_SAFETY_LIMIT` capped the fold's in-txn log read (and the pending /
unsettled / settled listings) at 5000 ops, silently dropping the **newest** ops
from the fold when an account exceeded it — a phantom inside the atomicity
core. The cap is deleted; `collect_operations` no longer takes a `limit`, and
all five reads (`list_pending_operations`, `list_unsettled_operations`,
`list_settled_operations`, `derive_overlay`, `remove_op_and_derive`) read the
full account log. Chosen over paging/backpressure for simplicity and
correctness; the O(rows × ops) cost during sync is the accepted trade-off
(realistic backlogs are tiny; a pathologically large outbox is a root-cause
problem — stuck flusher / no sync — to fix separately, not by truncating the
fold).

### F7. ✅ LANDED — content-kind list single-sourced

The SQL fragment and the Rust predicate now both derive from
`OperationKind::CONTENT_KINDS`; the compiler keeps them in lockstep (see
commit `dd1589b9`). Kept here for history.

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

### F8. ✅ LANDED — apply-mutation match extracted

`apply_overlay_mutation_tx` (prod) and `apply_mutation_to_overlay_map` (test)
share the mutation→write mapping; a new `OverlayMutation` variant lands in one
place per plane (commit `dd1589b9`). Kept here for history.

`match mutation { Upsert(record) => upsert_overlay_tx(…), Tombstone => …,
Remove => …, Keep => {} }` plus the `now_visible` derivation appears in both
`derive_overlay` (`overlay.rs:153-163`) and `remove_op_and_derive`
(`overlay.rs:213-223`), and twice more in the test store
(`store_sync_cache_impls.rs`). A new `OverlayMutation` variant must be
added at every site.

**Fix:** extract `apply_overlay_mutation_tx(tx, account_id, row_id, mutation)
-> bool` (returns `now_visible`) shared by both derives; the test store gets
a matching helper.

### F9. ✅ LANDED — `replay_account_overrides` wired to the post-repair path

It was load-bearing, not dead: a quarantined-and-rebuilt store salvages the
op log and `draft_alias` map but starts with an EMPTY overlay, and the only
thing that re-derived parked content (an unsent draft, a provisional Sent row
with no base successor) was the post-sync sweep — which runs only at a
completed cycle's end. `AppState::assemble` now replays each account's
overrides when `open_with_repair` reports a repair, before starting the
supervisor, so the derived view reappears immediately. Tested with a
disabled-account continuity test (the supervisor skips a disabled account, so
no sync/flush runs — isolating the wiring).

**Fix:** either wire it to the quarantine-and-rebuild path the model
specifies (L2-optimism "Durability classes"), or delete it and document that
the per-row `replay_base_write` + sweep is the only recovery. Do not leave it
ambiguous.

### F10. ✅ LANDED — design-history comments rewritten

The three comments now state invariants directly (commit `dd1589b9`). Kept
here for history.

`tests/replay.rs:1415` and `:1505` (and one more) explain the new behavior by
contrast with the retired non-atomic / clock-based fold. The house rule is
that comments describe the current design, not its history — these age into
folklore and mislead once the "old" code is forgotten.

**Fix:** rewrite them to state the invariant directly ("the fold is a pure
function of (log, base); a Pending send with an elapsed but undispatched hold
stays held…") without the comparative frame.

## P3 — echo / TOCTOU (self-healing, plumbing exists)

### F4–F5. ✅ LANDED — truncate echo uses effective visibility

`DeriveDiff` now carries both `retired()` (overlay-only — for a provisional id
with no base successor, e.g. adoption) and `effectively_retired()` (base ∪
overlay — for an id that may have a base successor, e.g. truncation). The
truncate site uses `effectively_retired()`, so a base-backed content row no
longer emits a spurious/double `deleted` echo. Adopt keeps `retired()` (no-base
provisional id — equivalent). See commit `3491fab4`. A dedicated base-caught-
up content-op truncate test is still wanted (the TestStore's global base
pretense makes a precise one subtle).

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

### F11. ✅ LANDED — echo visibility-decision sites use the in-txn `DeriveDiff`

The five echo sites that used `get_message_summary` for a VISIBILITY decision
(the TOCTOU reads) now use `refresh_message_overlay(...).effectively_retired()`
from the in-txn fold: the three before/after TOCTOU sites (cancel-draft-save,
unwind-send-row, held-send-consumed in `queue.rs`) and the two after-only
retire sites (rotation old-key, settle old-id in `draft.rs`). Closes the
race and drops ~7 effective-view reads.

The projection-echo sites KEEP a single `get_message_summary` read — they
serialize the `MessageSummary` as the event's `projection`, and that type
carries joined account/conversation/version fields the overlay-plane derive
cannot produce. `delete_draft` keeps its before-read for the deleted echo's
mailbox scope. These are content reads, not visibility decisions — no TOCTOU.

`refresh_message_overlay` already returned `DeriveDiff` (every caller ignored
it); the echo sites now use it, so no port change was needed. Replaces the
old "Deferred" item.

These three (`outbox/queue.rs`, `outbox/draft.rs`) were not converted to
`DeriveDiff` in the refactor — they read the effective view before and after
a refresh, a TOCTOU under concurrency (self-healing on the next replay, but
real). The plumbing exists: `refresh_message_overlay` returns `DeriveDiff`.

**Fix:** convert the three sites to `refresh_message_overlay(...).await?.retired()`
(or `effectively_retired()` once F4–F5 lands), deleting the duplicated
before/after reads. This is the last echo-cleanup site.
