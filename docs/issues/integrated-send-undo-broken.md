---
title: "Send and undo broken in the integrated app"
modified: 2026-07-18
state: resolved
---

# Send and undo broken in the integrated app

Observed in dogfood on v0.6.0-nightly.1: composing + sending does not
deliver, and undo does not act.

## Resolution (2026-07-18, charter slice 4 end-to-end proof)

All three flows were driven END-TO-END in the real app (dev backend over a
seeded Stalwart + `client:dev`, browser driven via playwright) and now pass;
the fixes below are gated by `tests/api.rs` regressions that fail without
them.

**Send — WORKS, and the phantom-draft leak it left behind is fixed.** The UI
verb posts the held-send envelope, the outbox holds it (`pending`,
cancelable), the scheduled tick flushes it, Stalwart confirms
`delivery.completed` + DSN 250, and the copy files to Sent. (A self-send's
inbox copy is server-deduped — same RFC `Message-ID` as the Sent copy — so
"did not deliver" can be misread from a self-send test.) FIXED underneath:
the held send's eager ensure-draft rotates the compose key to a provider id
but never retired the admission-time row pinned under the KEY — a phantom
Drafts row that survived the send's settlement forever (and duplicated the
restored draft after an undo). The rotation now retires the key row via the
same helper the draft-save settlement uses
(`outbox/draft.rs::retire_rotated_draft_row`). Regression:
`api.rs::held_send_rotation_retires_the_compose_key_row`.

**Undo-send — WORKS: cancel + restore both fixed.** Clicking the toast's
Undo cancels the held op (exactly one winner vs the flusher), nothing
delivers, and the draft is restored once. FIXED: the reopen-on-undo composer
showed "Could not prepare this message" — it addresses the restored draft by
its COMPOSE KEY, but `messageDetail` never resolved the key to the live
provider-id row (`get_draft_content` had the resolution but is not exposed
over the API). `messageDetail` now resolves through
`MailService::resolve_live_message_id` (shared with `get_draft_content`).

**Action undo (archive + toast Undo) — was DOUBLY broken, both fixed.**
1. The rev-log had readers (`revLog`, `undo`, `redo`) but NO writer: forward
   actions were never recorded (recording died with the split-model stack's
   retirement), so every undo hit an empty log — "nothing to undo". The two
   reversible mutations (`setKeywords`, `replaceMailboxes`) now record their
   EFFECTIVE pre/post delta server-side with the cursor auto-advance
   (`api/mail_mutations.rs`).
2. With recording alive, the toast Undo undid the WRONG step: selecting the
   next message auto-marks it read, and that implicit step claimed the
   cursor. `SetKeywordsIntent` gained `recordUndo` (absent = true); the
   auto-mark-read path sends `false`, so implicit read-state never enters
   the history (and never truncates the redo tail). Regression:
   `api.rs::forward_mutations_record_rev_log_steps_and_undo_reverts_them`.

Prior bisect results (still true): the toast's per-account
`{ undo: { accountId } }` routing (`undoToastOptions`), and the proven-green
envelope/verb/backend legs
(`api.rs::frontend_send_envelope_holds_flushes_and_cancels_by_command_id`,
`live_stalwart.rs::live_frontend_shaped_held_send_delivers_and_cancels`).

Not covered here: the dogfood account's Gmail/IMAP driver (the live gate is
Stalwart/JMAP); if dogfood still misbehaves there, the failure is now
provably inside the provider adapter.
