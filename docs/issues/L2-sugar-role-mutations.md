---
scope: L2
summary: "archive/trash/restoreToInbox are vestigial sugar over moveToRole — the authority dispatches each to move_message_to_role with a hardcoded role and adds no extra semantics (no read-marking, expunge, or \\Deleted). The web client already calls moveToRole for these actions. Collapse the three named mutations into moveToRole; keep destroy (genuinely distinct — permanent deletion) and moveToMailbox (by-id addressing)."
modified: 2026-06-27
reviewed: 2026-06-27
lifecycle: ephemeral
type: ISSUE
status: done
priority: medium
depends:
  - path: docs/runtime/mutations/L1
---

# Collapse the sugar role mutations into moveToRole

**Status: DONE** — folded on `main` right after the `.21` move-flicker fixes.

## The redundancy

`message.archive` / `message.trash` / `message.restoreToInbox` are 1:1 aliases
of `message.moveToRole` with a hardcoded role. The authority backend
(`backend.rs::apply_named_message_mutation`) dispatches each straight to
`move_message_to_role` with the literal role string — **no extra semantics**:

```rust
MessageMutation::Archive(args)        => self.move_message_to_role(account, message, "archive"),
MessageMutation::Trash(args)          => self.move_message_to_role(account, message, "trash"),
MessageMutation::RestoreToInbox(args) => self.move_message_to_role(account, message, "inbox"),
MessageMutation::MoveToRole(args)      => self.move_message_to_role(account, message, args.role),
```

No read-marking, no expunge, no `\Deleted`, no retention — a plain mailbox move.
The optimism layer already proved this: `to_assertion_with_roles` resolves all
four to the same `ReplaceMailboxes` assertion via the role→mailbox-id map. And the
web client already calls `moveToRole` for archive/trash
(`useEmailActions.ts` → `moveToRole(target, MAILBOX_ROLES.Archive, …)`) — the
named mutations have **no callers**. They are vestigial.

`docs/runtime/mutations/L1.md` even documents them as aliases ("Alias of
`moveToRole(role=archive)`"), so the fold aligns the code with the docs.

## What stays

- **`message.destroy`** — genuinely distinct (`destroy_message`, permanent
  deletion/expunge). A move preserves the message in another mailbox; destroy
  erases it. Not a role move.
- **`message.moveToMailbox`** (by mailbox id) vs **`message.moveToRole`** (by
  role) — different addressing, both kept.

## The fold

- `posthaste-link-contract`: drop the `Archive`/`Trash`/`RestoreToInbox`
  variants and their `from_request` / `account_id` / `message_id` /
  `to_assertion_with_roles` arms. `MessageTargetArgs` stays (used by `Destroy`).
- `posthaste-authority-runtime`: drop the three dispatch arms.
- Tests: re-point the role-resolution tests at `message.moveToRole`.
- Docs (`docs/runtime/mutations/L1.md`, `docs/client/L2.md`): drop the alias
  rows; the catalog lists `moveToRole` once.
- No web-client change (it already uses `moveToRole`).

## Why it's a deliberate (small) contract break, not a drive-by

Removing the names from `from_request` means a caller of `message.archive`
gets "unknown runtime mutation." No production caller does (verified
repo-wide), and the user-facing verbs ("Archive"/"Trash" buttons) are a thin
client wrapper over `moveToRole`. Done as a dedicated, self-contained diff so
the contract change is intentional and reviewable, not folded into the flicker
work.

## Provenance

Raised by the user after the `.21` flicker fixes: "Why do we have a separate
mutation for archive or destroy, are those just moving messages to mailboxes
Trash and Archive? This seems excessive and unnecessary." Investigation
confirmed archive/trash/restoreToInbox are sugar; destroy is not.
