---
scope: L2
summary: "Snooze: defer a message to a Posthaste-managed 'Snoozed' mailbox with a server-owned scheduler that returns it to the Inbox at a chosen time. Uniform across providers (Gmail-mirror investigated + rejected — Gmail's snooze isn't reachable via IMAP/JMAP). The snoozed_until timestamp is Posthaste-local metadata; the scheduler rides the existing supervisor tick loop; snooze is a user-initiated mutation (records an undo step) while the scheduler's auto-return is not."
modified: 2026-06-28
reviewed: 2026-06-28
lifecycle: ephemeral
type: DESIGN
status: "Design doc. Gmail-mirror investigated (not clean → uniform). Implementation pending: mailbox role + snoozed_until field, snooze/unsnooze mutations, scheduler tick, UI, undo integration."
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
  The "Snoozed" mailbox is provisioned per-account alongside Inbox/Archive/etc.
- `snoozed_until: Option<i64>` (unix seconds, UTC) on the message record.
  **Posthaste-local metadata** — providers have no snooze-until field, so the
  sync layer (which maps known provider fields) must not overwrite it. The
  field rides the existing message record/wire → openapi + schema.gen.ts regen
  (drift guards fire; backward-compatible optional field).
- A snoozed message is in the `Snoozed` mailbox (so it's naturally hidden from
  the Inbox view) **and** carries `snoozed_until`. The mailbox is the
  user-visible "where is it"; `snoozed_until` is the scheduler's trigger.

### Mutations

- `message.snooze` (`{ messageId, until: i64 }`): move to the Snoozed mailbox +
  set `snoozed_until = until`. **User-initiated** (`context.userInitiated =
  true`) → records a rev_log undo step (Slice 5d gate). The captured diff
  includes **both** the mailbox change and the `snoozed_until` change, so undo
  restores the message to its prior mailbox with `snoozed_until = null`.
- `message.unsnooze` (`{ messageId }`): move back to the Inbox + clear
  `snoozed_until`. User-initiated → records an undo step. (A plain
  `moveToRole(Inbox)` would move the message but leave `snoozed_until` set, so a
  dedicated unsnooze mutation is cleaner than reusing the move path.)

### Scheduler (server-owned, cross-device)

The authority-runtime supervisor already runs a per-account tick loop
(`supervisor/runtime.rs`: `oauth_refresh_interval`, `backfill_interval`,
`cache_interval` via `tokio::time::interval_at`). Add a **snooze tick** (e.g.
every 60s): `SELECT id FROM message WHERE mailbox = Snoozed AND snoozed_until
<= now` → for each, move to Inbox + clear `snoozed_until`.

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

- `MailboxRole::Snooze` → domain enum → openapi (the role is a string in
  several response schemas) + schema.gen.ts.
- `snoozed_until` on the message record → message wire schema → openapi +
  schema.gen.ts + the store SQL schema (a `snoozed_until INTEGER` column +
  migration).
- `message.snooze` / `message.unsnooze` mutations → the mutation catalog
  (`runtime/mutations/L1`) + the command wire (`/commands/messages/{id}/snooze`).
- `Regenerate after intentional API changes with UPDATE_OPENAPI=1 ...` then
  `bun run api:generate` (the established flow).

## Phased implementation

- **Slice 1 — model + storage**: `MailboxRole::Snooze` + `snoozed_until` column
  + domain field + provisioning. openapi/schema regen.
- **Slice 2 — mutations**: `message.snooze` + `message.unsnooze` (command wire
  + backend apply + projection). User-initiated tagging.
- **Slice 3 — scheduler**: snooze tick in `supervisor/runtime.rs` + the due-row
  query + auto-return.
- **Slice 4 — UI**: Snooze button + popover/presets in the message header;
  sidebar entry for the Snoozed mailbox; `useEmailActions` wiring.
- **Slice 5 — undo integration + e2e**: confirm the snooze diff captures both
  fields; verify undo restores; Playwright e2e (snooze → assert leaves Inbox +
  appears in Snoozed → undo → back in Inbox; advance the scheduler / wait →
  auto-return).

## Open questions / follow-ups

- **Alias self-exclusion** (carried from reply-all): `useComposeFormState`
  excludes only the primary identity email; sender-address aliases are a
  follow-up. (Unrelated to snooze but noted.)
- **Gmail native-snooze interop**: if a Gmail user snoozes via Gmail's app, the
  message moves to Gmail's internal Snoozed state (invisible to IMAP). We
  won't see it until Gmail returns it. Acceptable (matches other clients); a
  future "resync detected Gmail-side moves" pass could surface it.
- **Snooze + undo across the scheduler boundary**: if a user undoes a snooze
  after the scheduler already auto-returned it, undo applies the reverse diff
  (move to Snoozed + restore `snoozed_until`). The message goes back to
  Snoozed with a now-past `snoozed_until` → the scheduler re-returns it on the
  next tick. This is correct (undo is a point-in-time restore; the scheduler
  re-converges) but worth an e2e to confirm the user isn't surprised.
