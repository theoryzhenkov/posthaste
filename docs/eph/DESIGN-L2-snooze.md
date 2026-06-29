---
scope: L2
summary: "Snooze: defer a message to a Posthaste-managed 'Snoozed' mailbox with a server-owned scheduler that returns it to the Inbox at a chosen time. Uniform across providers (Gmail-mirror investigated + rejected — Gmail's snooze isn't reachable via IMAP/JMAP). The return time lives in a separate `message_snooze(account_id, message_id, until)` store table (Posthaste-local, not provider-synced; no message-wire change); the scheduler rides the existing supervisor tick loop; snooze is a user-initiated mutation (records an undo step) while the scheduler's auto-return is not. A store invariant — leaving the Snoozed mailbox deletes the snooze row — makes undo correct without the diff capturing the snooze-table change."
modified: 2026-06-28
reviewed: 2026-06-28
lifecycle: ephemeral
type: DESIGN
status: "Implementation in progress (Option B: separate `message_snooze` table). Slices 1–4 shipped; Slice 5 undo-integration verified (applyDiff → replace_mailboxes → invariant clears the row, locked with an authority-runtime test) + e2e caught a JMAP designation gap (set_mailbox_role gateway round-trip rejects 'snooze'; needs a local-override path for non-provider roles)."
depends:
  - path: docs/eph/DESIGN-L2-undo-redo-revlog-contract
    note: "undo integration — snooze is a user-initiated mutation that records a rev_log step; the scheduler's auto-return must not"
  - path: docs/runtime/mutations/L1
    section: "1. Mutation pipeline and catalog"
  - path: crates/posthaste-domain/src/vocab
    note: "MailboxRole enum — add Snooze"
---

# Snooze

## Goal

Let a user defer a message: it leaves the Inbox now and reappears at a chosen
later time. Cross-device (the snooze state is server-owned account state, so
every client sees it). Undoable (snooze records an undo step; undo restores it
to the Inbox immediately).

## Gmail-mirror: investigated, rejected

Gmail has a native snooze (a "Snoozed" system label + a server-side scheduler).
Mirroring it for Gmail accounts would let Gmail own the scheduler. **Not
clean:**

- The "Snoozed" label is **not exposed via IMAP** — it's an internal label Gmail
  doesn't surface to IMAP clients ([SO 71594360](https://stackoverflow.com/q/71594360)).
- Gmail's scheduler **only fires for snoozes set via Gmail's own web/iOS/Android
  app** — there is no IMAP/JMAP operation to set a snooze return time. Third-party
  clients that implement their own snooze (e.g. eM Client) confirm Gmail does
  not sync it: a third-party snooze leaves the message in the Gmail Inbox
  ([emclient forum](https://forum.emclient.com/t/snoozed-emails-stay-in-gmail-inbox/84497)).

So per the "if clean" gate → **uniform Posthaste snooze for all providers**,
including Gmail. This matches how every other third-party client does it.

## Design

### Mailbox role + storage

- `MailboxRole::Snooze` (serialize `"snooze"`) in `posthaste-domain/vocab.rs`.
  The "Snoozed" mailbox is **not** Posthaste-auto-provisioned — there is no
  gateway `create_mailbox`, + mailboxes come from provider sync. Instead it's a
  **provider mailbox the user designates with the `snooze` role** via the
  existing `mailbox_role_override` (the same role-switch the SourceMailboxEditor
  uses). The snooze mutation looks up the mailbox with role `snooze` + moves the
  message there (provider-side, via the existing move machinery) — clean for
  cross-device (the move is on the provider; all clients see it via sync) + no
  resync-clobber. If no mailbox has the `snooze` role, the snooze mutation
  rejects with a clear error (the UI prompts the user to designate one). v1
  doesn't auto-create/auto-designate; that's a follow-up (would need a gateway
  `create_mailbox`).
  > **Gap (found by the Slice 5 e2e):** `set_mailbox_role` does an unconditional
  > gateway round-trip (`gateway.set_mailbox_role`), so designating a mailbox
  > with the `snooze` role FAILS for JMAP providers — JMAP's mailbox `role`
  > property only accepts standard roles (inbox/archive/drafts/sent/junk/trash),
  > so Stalwart rejects `"snooze"` (`gateway_rejected: invalidProperties: role`).
  > The local `mailbox_role_override` table exists but `set_mailbox_role` never
  > writes it. Fix direction: write the local `mailbox_role_override` + skip the
  > gateway round-trip for non-provider roles (like `snooze`). Until fixed, the
  > snooze UI can't designate a Snoozed mailbox on JMAP accounts.
- A **separate `message_snooze` store table**
  `(account_id TEXT, message_id TEXT, until INTEGER NOT NULL, PRIMARY KEY (account_id, message_id))`
  with an index on `(account_id, until)` for the scheduler. The return time is
  **Posthaste-local metadata** — providers have no snooze-until field, so the
  sync layer (which maps known provider fields) never touches this table. The
  message record/wire is **unchanged** (no `snoozedUntil` field), so there's no
  message-schema/openapi churn — only the `MailboxRole` enum gains the `snooze`
  value.
- A snoozed message is in the `Snoozed` mailbox (so it's naturally hidden from
  the Inbox view) **and** carries a `message_snooze` row. The mailbox is the
  user-visible "where is it"; the snooze row is the scheduler's trigger.

### Mutations

- `message.snooze` (`{ messageId, until: i64 }`): move to the Snoozed mailbox +
  insert the `message_snooze(account_id, message_id, until)` row. **User-initiated**
  (`context.userInitiated = true`) → records a rev_log undo step (Slice 5d gate).
  The captured entity-store diff is the mailbox change (the snooze row is in a
  separate table, not the replica); undo restores the mailbox, + the store
  invariant (below) clears the snooze row as a side effect.
- `message.unsnooze` (`{ messageId }`): move back to the Inbox + delete the
  `message_snooze` row. User-initiated → records an undo step.
- **Store invariant**: whenever a message leaves the `Snoozed` mailbox (by any
  path — unsnooze mutation, undo restoring the prior mailbox, a manual move, or
  the scheduler's auto-return), the `message_snooze` row is deleted. This makes
  undo correct without the diff having to capture the snooze-table change:
  undo applies the reverse mailbox diff (move back), the invariant clears the
  snooze row, + the scheduler no longer sees it. The only way to *enter* Snoozed
  with a return time is the `message.snooze` mutation (a plain move to Snoozed
  via `moveToRole`/drag is rejected or leaves no row — the Snoozed mailbox is
  action-gated, not a drop target).

### Scheduler (server-owned, cross-device)

The authority-runtime supervisor already runs a per-account tick loop
(`supervisor/runtime.rs`: `oauth_refresh_interval`, `backfill_interval`,
`cache_interval` via `tokio::time::interval_at`). Add a **snooze tick** (e.g.
every 60s): `SELECT message_id FROM message_snooze WHERE account_id = ? AND
until <= now` → for each, move to Inbox + delete the snooze row (the auto-return
path).

- **Server-owned** → one place, cross-device coherent (any client sees snooze
  state via sync).
- **Not user-initiated** → the auto-return does not record an undo step (the
  `userInitiated` gate excludes it). A user undoing a snooze gets it back
  immediately; if the scheduler later fires for a since-unsnoozed message, the
  query simply finds nothing (idempotent — the message is no longer in the
  Snoozed mailbox).
- `MissedTickBehavior::Skip` (matches the existing ticks) so a long pause
  doesn't fire a burst.

### UI

- A Snooze button in the message header (next to Reply All) + a small popover
  with preset times (Later today / Tomorrow / This weekend / Next week) + a
  custom datetime picker. Mirrors the existing archive/trash action shape
  (`useEmailActions` → `runtimeMutations.messages.snooze`, tagged
  `userInitiated`).
- The Snoozed mailbox appears in the sidebar (like Archive/Trash) so users can
  see + manually unsnooze.

## Wire / artifact churn

- `MailboxRole::Snooze` → domain enum → openapi (the role is a string enum in
  several response schemas) + schema.gen.ts (the `snooze` value).
- The `message_snooze` table is store-internal — no message wire/schema change
  (that's the point of Option B). The snooze/unsnooze commands ride the named-
  mutation pipeline; their args (`until`) are in `MutationRequest.context`,
  which already carries arbitrary payloads with zero wire churn.
- `message.snooze` / `message.unsnooze` mutations → the mutation catalog
  (`runtime/mutations/L1`).
- `Regenerate after intentional API changes with UPDATE_OPENAPI=1 ...` then
  `bun run api:generate` (the established flow) — for the `MailboxRole` enum
  only.

## Phased implementation

- **Slice 1 — model + storage**: `MailboxRole::Snooze` + `message_snooze` table
  + index + provisioning. openapi/schema regen (MailboxRole enum).
- **Slice 2 — mutations + store invariant**: `message.snooze` +
  `message.unsnooze` named mutations (command wire + backend apply + the snooze
  row insert/delete). Store invariant: leaving Snoozed → delete the snooze row.
  User-initiated tagging.
- **Slice 3 — scheduler**: snooze tick in `supervisor/runtime.rs` + the
  due-row query + auto-return.
- **Slice 4 — UI**: Snooze button + popover/presets in the message header;
  sidebar entry for the Snoozed mailbox; `useEmailActions` wiring.
- **Slice 5 — undo integration + e2e**: confirm the store invariant clears the
  snooze row on undo (✅ verified — `applyDiff` → `replace_mailboxes` → the
  invariant fires; locked with an authority-runtime test). Playwright e2e
  (snooze → assert leaves Inbox + appears in Snoozed → undo → back in Inbox;
  advance the scheduler / wait → auto-return) — ⚠️ blocked on the JMAP
  designation gap above (can't designate a Snoozed mailbox on the dev stack's
  Stalwart account until `set_mailbox_role` gets a local-override path).

## Open questions / follow-ups

- **Alias self-exclusion** (carried from reply-all): `useComposeFormState`
  excludes only the primary identity email; sender-address aliases are a
  follow-up. (Unrelated to snooze but noted.)
- **Gmail native-snooze interop**: if a Gmail user snoozes via Gmail's app, the
  message moves to Gmail's internal Snoozed state (invisible to IMAP). We
  won't see it until Gmail returns it. Acceptable (matches other clients); a
  future "resync detected Gmail-side moves" pass could surface it.
- **Snooze + undo across the scheduler boundary**: if a user undoes a snooze
  after the scheduler already auto-returned it, undo applies the reverse mailbox
  diff (move to Snoozed) — + the store invariant does NOT insert a snooze row
  (entering Snoozed via undo is not a `message.snooze` mutation, so there's no
  return time). The message ends up in Snoozed with no auto-return. This is
  arguably correct (undo is a point-in-time restore: the user was undoing the
  snooze, the scheduler already returned it, undo puts it back where it was
  mid-snooze) — but the message is stuck in Snoozed until the user manually
  unsnoozes or re-snoozes. Worth an e2e to confirm + maybe a UX call (should
  undo-of-snooze-after-auto-return just no-op, or restore to Snoozed?). The
  invariant direction matters: *leaving* Snoozed clears the row (clean);
  *entering* Snoozed via undo does not insert a row (no return time).
