# RFC-L2: Mailbox management + sidebar Groups

> Net-new feature (post-v0.3.0). Create/delete server-side JMAP+IMAP mailboxes, and
> client-side, cross-device-synced "Groups" that visually cluster mailboxes in the
> sidebar. Grounded in the exploration map (2026-07-06).

## Decisions (owner-confirmed)
- **Groups sync across devices** — stored in `AppSettings` (replica-persisted, TOML source of truth), like the existing `mailbox_colors` / `smart_mailbox_order`. NOT tab-local `client-preferences`.
- **Delete = confirm-with-count** — the UI shows "this permanently deletes N messages" and requires explicit confirmation; then JMAP `onDestroyRemoveEmails=true` / IMAP `DELETE`.
- **Post-v0.3.0** — the stable beta is cut; this is the first feature update.

## Architecture

### A1 — Mailbox mutations are SYNCHRONOUS, not optimistic
The optimistic outbox is message/draft-scoped: `MailOperation`/`OperationEntityKind` (`Message|Draft`) + the `MessageAssertion` fold have **no mailbox entity**. Making create/delete optimistic means a whole new op category + fold — disproportionate for a rare operation. Instead **clone the existing synchronous `set_mailbox_role` pattern** (`MailService::set_mailbox_role` → `gateway.set_mailbox_role` → `sync_account` readback): a blocking provider round-trip, then a resync to reflect the change. Clean ports/adapters, right-sized. UI shows a brief pending state; no offline mailbox mutation (acceptable).

### A2 — The port + provider impls
- New `MailGateway` port methods `create_mailbox(name)` and `destroy_mailbox(id, remove_emails)` (mirroring `set_mailbox_role`, `ports/gateway.rs`).
- **JMAP** (`live_mutation/requests.rs` + `live/gateway.rs`): hand-rolled `Mailbox/set` **create** (name, no parent — flat) and **destroy** (with `onDestroyRemoveEmails`). The `jmap-client` dep already exposes `on_destroy_remove_emails` / create; `outcome.rs` already parses `MailboxSetResponse`.
- **IMAP** (`posthaste-imap/src/mailbox.rs` — today only STATUS/SELECT): new `CREATE` and `DELETE`. IMAP `DELETE` destroys contained mail server-side and has **no `onDestroyRemoveEmails` equivalent** → the non-empty guard is enforced in the SERVICE layer (require the explicit confirmed flag), not the provider.
- Settle/cleanup: resync (`sync/mailbox.rs`); local row teardown reuses `mutations/mailbox_cleanup.rs`.

### A3 — Cross-boundary wiring (the checklist)
Each new service method threads through, mirroring the 7 `set_mailbox_role` call sites:
REST (`http-api-adapter/api/mailboxes.rs`: `POST /v1/mailboxes`, `DELETE /v1/mailboxes/{id}?removeEmails=`) → `runtime-api` trait → `authority-server-link` → `authority-server` command → regenerate `apps/web/src/api/schema.gen.ts` + `events.gen.ts` + `apps/mcp/src/schema.gen.ts` → MCP tool (`apps/mcp` create/delete mailbox) → web `runtimeMutations`.

### A4 — Groups (client-side, synced)
- Model: `AppSettings.mailbox_groups: Vec<MailboxGroup>` where `MailboxGroup { id, name, mailbox_ids: Vec<String>, order }` (`domain-model/account_settings.rs`). Purely presentational — a Group never maps to a provider parent/child mailbox (nesting is out of scope).
- Persist via the existing settings path: web `runtimeMutations.settings.patch` → `PATCH /v1/settings` (`PatchSettingsRequest`, already carries `mailbox_colors`) → `patch_app_settings`. Syncs cross-client for free.
- UI: `sidebar/SourceSection.tsx` renders each Group as a collapsible wrapper around its member mailboxes; ungrouped mailboxes render flat as today. Create/rename/delete a group + assign/remove mailboxes (context menu). Thread Groups into the j/k `navItems` walker (`Sidebar.tsx`) and the `collapsedSourceIds` collapse infra.

## Slices
- **M1 — Mailbox create** (service + port + both providers + full boundary + sidebar "New mailbox" affordance + resync).
- **M2 — Mailbox delete** (service + providers + boundary + confirm-with-count dialog + local cleanup). Service refuses destroy without the explicit confirmed `remove_emails` flag.
- **M3 — Groups** (AppSettings field + settings boundary regen + sidebar Group wrapper + create/assign UI + j/k threading). Independent of M1/M2 except the shared sidebar; sequence after M1/M2 or split sidebar territory carefully.

## Out of scope (future)
Nested mailboxes (model/store/read-model are all flat — a separate schema+sync change); IMAP RENAME / hierarchy delimiter; mapping Groups to real provider parent mailboxes.

## Risks
IMAP has zero server-side mailbox mutation today (all net-new) + no non-empty guard (service-enforced). Cross-boundary surface is wide (REST+runtime-api+authority+codegen+MCP). Sidebar j/k walker must stay correct with the new Group nesting level.
</content>
