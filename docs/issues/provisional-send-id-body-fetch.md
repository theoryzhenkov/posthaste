---
title: "Body fetch errors on a provisional send-<id>"
modified: 2026-07-20
state: resolved
---

# Body fetch errors on a provisional send-<id>

Observed in dogfood on v0.6.0-nightly.5: after sending a message, a
"Something went wrong — gateway rejected the request: missing IMAP location
for message send-<uuid>" toast appears with no user action beyond waiting for
the send to dispatch. "Load failed" accompanies it. Regular send (no undo,
no delete) still surfaced the error.

## Root cause

`send-<uuid>` is the **provisional Sent row** — the overlay-only row a
dispatched-but-unadopted send surfaces (`replay.rs::synthesize_sent_record`,
keyed by the send op's entity id `format!("send-{}", Id::generate())`). It has
**no IMAP message** behind it: the real copy lands under its own provider id
and `adopt_sent_copies` retires `send-<id>` by matching the RFC-`Message-ID`
prefix. Until adoption, `list_imap_message_locations(send-<id>)` is empty.

`MailService::get_message_detail` (`message_queries.rs`) lazily fetches the
body via `gateway.fetch_message_body` when the detail's body isn't loaded.
For a `send-<id>`, that call reaches `location_and_mailbox_name`
(`planning.rs`), finds no locations, and rejects with `GatewayError::Rejected`
— which the frontend's default query `onError` (`notifyFromError`) surfaces as
the "Something went wrong" toast. The body cache worker was ruled out: it
only fetches base messages (the sync sink accumulates `batch.messages`, not
projected rows), so `send-<id>` never becomes a cache candidate.

This is **pre-existing**, not a regression from the replay-engine refactor
(`68d7890f`→`316d85ca`). The refactor's `send_is_held` change (Slice 2)
actually *narrows* the window: the provisional Sent row appears only after
dispatch, not while merely past-due.

## Resolution (2026-07-20)

`get_message_detail` now skips the body fetch for a provisional `send-<id>`
and returns the detail without a body — it becomes available under the real
id once adoption retires the provisional row. The gateway stays correct to
reject a genuinely locationless real id; the carve-out is only for the
provisional-Sent case.

- `posthaste_domain_model::is_provisional_sent_id` / `SEND_ENTITY_ID_PREFIX`
  (`model/outbox.rs`) — the single authority for the provisional prefix,
  used at the send-id generation site (`outbox/queue.rs`) and the detail
  query.
- `MailService::get_message_detail` (`message_queries.rs`) — early-return
  the detail without a body when `is_provisional_sent_id(message_id)`.

Regression: `message_queries::get_message_detail_skips_body_fetch_for_a_provisional_send_row`
(fails without the fix — the gateway rejects; passes with it — the body fetch
is never called). The `TestStore::get_message_detail` mock now reads the
overlay (mirroring the real effective read) so the detail path is exercisable.

## Not covered here

A `Destroy`/`ReplaceMailboxes`/`SetKeywords` op enqueued against a
`send-<id>` (a user delete/trash/archive/flag on a not-yet-adopted sent
message) still fails at push with the same "missing IMAP location" — that
needs the adoption alias bridge (send-`<id>` → real provider id, resolved at
flush, mirroring `resolve_draft_flush_target` for drafts). It is a separate,
user-action-gated path; this fix addresses the no-user-action toast.

**Update (2026-07-18):** the adoption alias bridge is now implemented — a
`send_alias` table + `SendRegistry` port record `send-<id>` → adopted real id
at adoption, and the flush retargets state-assertion ops to it (deferring while
the send is in flight, no-op-ing when it failed/was discarded). See
`feat(service): retarget state-assertion ops on a provisional send-<id>`.
