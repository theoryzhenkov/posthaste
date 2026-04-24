---
scope: L1
summary: "REST endpoint contracts, request/response schemas, error codes, SSE event stream"
modified: 2026-04-24
reviewed: 2026-04-24
depends:
  - path: docs/L0-api
  - path: docs/L1-sync
  - path: docs/L1-jmap
dependents:
  - path: docs/L1-ui
---

# API domain -- L1

## Endpoint table

All endpoints are prefixed with `/v1`.

### Settings

| Method | Path | Handler | Request | Response |
|--------|------|---------|---------|----------|
| GET | `/settings` | `get_settings` | -- | `AppSettings` |
| PATCH | `/settings` | `patch_settings` | `PatchSettingsRequest` | `AppSettings` |

### Accounts

| Method | Path | Handler | Request | Response |
|--------|------|---------|---------|----------|
| GET | `/accounts` | `list_accounts` | -- | `AccountOverview[]` |
| POST | `/accounts` | `create_account` | `CreateAccountRequest` | `AccountOverview` |
| GET | `/accounts/{account_id}` | `get_account` | -- | `AccountOverview` |
| PATCH | `/accounts/{account_id}` | `patch_account` | `PatchAccountRequest` | `AccountOverview` |
| DELETE | `/accounts/{account_id}` | `delete_account` | -- | `OkResponse` |
| POST | `/accounts/{account_id}/verify` | `verify_account` | -- | `VerificationResponse` |
| POST | `/accounts/{account_id}/enable` | `enable_account` | -- | `OkResponse` |
| POST | `/accounts/{account_id}/disable` | `disable_account` | -- | `OkResponse` |
| POST | `/accounts/{account_id}/logo` | `upload_account_logo` | raw image bytes | `AccountOverview` |
| GET | `/account-assets/logos/{image_id}` | `get_account_logo` | -- | image bytes |

### Smart mailboxes

| Method | Path | Handler | Request | Response |
|--------|------|---------|---------|----------|
| GET | `/smart-mailboxes` | `list_smart_mailboxes` | -- | `SmartMailboxSummary[]` |
| POST | `/smart-mailboxes` | `create_smart_mailbox` | `CreateSmartMailboxRequest` | `SmartMailbox` |
| GET | `/smart-mailboxes/{id}` | `get_smart_mailbox` | -- | `SmartMailbox` |
| PATCH | `/smart-mailboxes/{id}` | `patch_smart_mailbox` | `PatchSmartMailboxRequest` | `SmartMailbox` |
| DELETE | `/smart-mailboxes/{id}` | `delete_smart_mailbox` | -- | `OkResponse` |
| POST | `/smart-mailboxes:reset-defaults` | `reset_default_smart_mailboxes` | -- | `SmartMailboxSummary[]` |
| GET | `/smart-mailboxes/{id}/messages` | `list_smart_mailbox_messages` | `ListSmartMailboxMessagesQuery` | `MessagePageResponse` |
| GET | `/smart-mailboxes/{id}/conversations` | `list_smart_mailbox_conversations` | `ListConversationsQuery` | `ConversationPageResponse` |

### Navigation

| Method | Path | Handler | Request | Response |
|--------|------|---------|---------|----------|
| GET | `/sidebar` | `get_sidebar` | -- | `SidebarResponse` |

`SidebarResponse` includes smart mailbox summaries, real tag summaries derived
from non-system JMAP keywords, and enabled account mailbox trees. Tag counts are
merged across enabled accounts and exclude system keywords such as `$seen` and
`$flagged`.

### Conversations and messages

| Method | Path | Handler | Request | Response |
|--------|------|---------|---------|----------|
| GET | `/views/conversations` | `list_conversations` | `ListConversationsQuery` | `ConversationPageResponse` |
| GET | `/views/conversations/{id}` | `get_conversation` | -- | `ConversationView` |
| GET | `/sources/{source_id}/mailboxes` | `list_mailboxes` | -- | `MailboxSummary[]` |
| PATCH | `/sources/{source_id}/mailboxes/{mailbox_id}` | `patch_mailbox` | `PatchMailboxRequest` | `MailboxSummary[]` |
| GET | `/sources/{source_id}/messages` | `list_source_messages` | `ListSourceMessagesQuery` | `MessagePageResponse` |
| GET | `/sources/{source_id}/messages/{id}` | `get_message` | -- | `MessageDetail` |

### Compose

| Method | Path | Handler | Request | Response |
|--------|------|---------|---------|----------|
| GET | `/sources/{source_id}/identity` | `get_identity` | -- | `Identity` |
| GET | `/sources/{source_id}/messages/{id}/reply-context` | `get_reply_context` | -- | `ReplyContext` |
| POST | `/sources/{source_id}/commands/send` | `send_message` | `SendMessageRequest` | `OkResponse` |

### Message commands

| Method | Path | Handler | Request | Response |
|--------|------|---------|---------|----------|
| POST | `/sources/{sid}/commands/messages/{mid}/set-keywords` | `set_keywords` | `SetKeywordsCommand` | `CommandResult` |
| POST | `/sources/{sid}/commands/messages/{mid}/add-to-mailbox` | `add_to_mailbox` | `AddToMailboxCommand` | `CommandResult` |
| POST | `/sources/{sid}/commands/messages/{mid}/remove-from-mailbox` | `remove_from_mailbox` | `RemoveFromMailboxCommand` | `CommandResult` |
| POST | `/sources/{sid}/commands/messages/{mid}/replace-mailboxes` | `replace_mailboxes` | `ReplaceMailboxesCommand` | `CommandResult` |
| POST | `/sources/{sid}/commands/messages/{mid}/destroy` | `destroy_message` | -- | `CommandResult` |

### Sync and events

| Method | Path | Handler | Request | Response |
|--------|------|---------|---------|----------|
| POST | `/sources/{source_id}/commands/sync` | `trigger_sync` | -- | `{ ok, eventCount }` |
| POST | `/config:reload` | `reload_config` | -- | `OkResponse` |
| GET | `/events` | `stream_events` | `EventsQuery` | SSE stream |

## Error format

All error responses are JSON objects with three fields:

```json
{ "code": "not_found", "message": "account not found", "details": {} }
```

### Error code mapping

| `ServiceError` code | HTTP status |
|---------------------|-------------|
| `not_found` | 404 |
| `conflict`, `state_mismatch` | 409 |
| `auth_error` | 401 |
| `gateway_unavailable` | 503 |
| `network_error` | 502 |
| `gateway_rejected`, `secret_unavailable`, `secret_unsupported` | 400 |
| `config_validation`, `config_parse` | 400 |
| `config_io` | 500 |
| (other) | 500 |

Request validation errors use handler-specific codes: `invalid_account`, `invalid_secret`, `invalid_cursor`, `invalid_limit`, `invalid_query`, `invalid_compose`, `invalid_mailbox`.

## Mailbox metadata

`PATCH /sources/{source_id}/mailboxes/{mailbox_id}` updates server-side mailbox metadata through JMAP `Mailbox/set`. The initial supported request field is `role`; valid values are `inbox`, `archive`, `drafts`, `sent`, `junk`, `trash`, or `null` to clear the role. When assigning a role that another mailbox currently owns, the server first clears the old owner, then assigns the new owner using the returned mailbox state. After the mutation succeeds, the server refreshes the account's mailbox projection and returns the current `MailboxSummary[]`.

## Cursor pagination

Conversation and message list endpoints accept `limit`, `cursor`, `sort`, `sortDir`, and `q` query parameters. The default limit is 100; the maximum is 250. A limit of 0 or above 250 returns `invalid_limit`.

Message list endpoints return `MessagePageResponse { items, nextCursor }`. They accept `q` as the same search query text used by the command/search panel. For source message lists, `q` is ANDed with the selected source and optional mailbox. For smart-mailbox message lists, `q` is ANDed with the saved smart-mailbox rule. Invalid query text returns `invalid_query`.

### Sort parameters

| Param | Type | Default | Values |
|-------|------|---------|--------|
| `sort` | `ConversationSortField?` | `date` | `date`, `from`, `subject`, `source`, `threadSize`, `flagged`, `attachment` |
| `sort` | `MessageSortField?` | `date` | `date`, `from`, `subject`, `source`, `flagged`, `attachment` |
| `sortDir` | `SortDirection?` | `desc` | `asc`, `desc` |

The backend sorts conversations by `(sort_key, conversation_id)` and messages by `(sort_key, source_id, message_id)` in the requested direction. For example, `sort=from&sortDir=asc` orders by sender ascending, breaking ties by stable IDs.

### Cursor format

The cursor is an opaque string. Conversation cursors encode the active sort value and conversation ID; message cursors encode the active sort value, source ID, and message ID. Clients must not inspect the format. The backend decodes the cursor and uses seek-based pagination to produce the next page. The response includes `nextCursor` if more results exist; `null` otherwise. Pages are strictly past the cursor in the current sort order, with no OFFSET-based skipping.

## SSE event stream

`GET /v1/events` opens a Server-Sent Events stream. Query parameters:

| Param | Type | Description |
|-------|------|-------------|
| `accountId` | string? | Filter events to a single account |
| `topic` | string? | Filter by event topic |
| `mailboxId` | string? | Filter by mailbox |
| `afterSeq` | integer? | Resume from this sequence number (exclusive) |

When `afterSeq` is provided, the backend replays matching events from the `event_log` table (backlog) before switching to the live broadcast stream. This allows the frontend to reconnect without missing events.

Each SSE event has `id` set to the event's sequence number and `data` set to the JSON-serialized `DomainEvent`.

The stream sends keepalive comments at the default Axum interval to prevent connection timeout.

## Account CRUD lifecycle

**Create**: `POST /accounts` accepts account name, optional full name, email address/pattern ownership, JMAP transport details, and a secret instruction. If `id` is omitted, the backend derives an internal unique ID from the first email pattern or account name. UI-created accounts always use JMAP. The endpoint applies the secret instruction, validates required fields (for JMAP: base URL and configured secret; username is optional for bearer-token auth), persists to config, starts the supervisor runtime, and emits an `account.created` event.

**Patch**: `PATCH /accounts/{id}` merges provided fields into the existing account. Omitted fields in the transport sub-object preserve their current values (sparse merge). Secret handling uses the backend `SecretWriteMode` tri-state: `keep` (preserve existing), `replace` (store new secret in keyring), `clear` (delete managed secret). The settings UI exposes this as an empty password field to keep the configured secret or a filled password field to replace it.

**Delete**: `DELETE /accounts/{id}` removes the managed OS keyring secret (if any), treating an already-missing keyring entry as deleted, stops the supervisor runtime, deletes the config file, and emits an `account.deleted` event.

**Verify**: `POST /accounts/{id}/verify` attempts a JMAP session discovery and returns whether the connection succeeded, the primary identity email, and whether push is supported.

**Enable/Disable**: Toggle `enabled` flag, re-persist, and restart the supervisor (which respects the flag).

**Appearance**: `AccountOverview` includes a resolved `appearance` object for the account mark. Account config may persist either `{ kind: "initials", initials, colorHue }` or `{ kind: "image", imageId, initials, colorHue }`. If no appearance is configured, the API derives initials and a stable hue from the account. `PATCH /accounts/{id}` can update letter/color appearance. `POST /accounts/{id}/logo` accepts raw PNG, JPEG, WebP, or GIF bytes up to 2 MiB, stores the image under the config root, updates account appearance to `image`, and returns the updated overview. Logo bytes are served from `GET /account-assets/logos/{image_id}`.

**Automation rules**: `AccountOverview`, `CreateAccountRequest`, and `PatchAccountRequest` include `automationRules`. Each rule has `id`, `name`, `enabled`, `triggers`, `scope`, `condition`, `actions`, and `backfill`. `condition` uses the same smart-mailbox rule tree as saved searches. PATCH replaces the full rule list when `automationRules` is present and preserves it when omitted. Rule IDs must be unique, enabled rules need at least one trigger and one action, tag actions must target non-system keywords, and mailbox IDs must be non-empty where used.

## Secret management

Account secrets are opaque authentication material. For JMAP accounts this may be an OAuth token set, a provider API token, or a development credential accepted by the provider. The API must not assume that the value is a Fastmail app-specific password.

Secrets use a tri-state write mode:

| Mode | Behavior |
|------|----------|
| `keep` | Preserve existing `secret_ref`; no secret value allowed |
| `replace` | Store the submitted secret value in OS keyring under `account:{id}` key; secret value required |
| `clear` | Delete managed OS secret; no secret value allowed |

The API never returns secret values. Responses include `SecretStatus` with `storage` (os/env), `configured` (bool), and `label` (env var name for env-type, redacted for os-type).

## Smart mailbox CRUD

**Create**: `POST /smart-mailboxes` generates an ID from the name (`sm-{slug}-{uuid}`), persists to config.

**Patch**: `PATCH /smart-mailboxes/{id}` merges name, position, and rule fields.

**Reset defaults**: `POST /smart-mailboxes:reset-defaults` restores all default smart mailboxes (Inbox, Archive, Drafts, Sent, Junk, Trash, All Mail) and returns the full list.

## Message body sanitization

`GET /sources/{source_id}/messages/{id}` sanitizes `body_html` through `sanitize_email_html` before returning to the frontend. This is the only place HTML is sanitized in the API layer; the sanitization runs in Rust before the response is serialized.

## Assertions

| ID | Sev. | Assertion |
|----|------|-----------|
| error-format | MUST | All error responses are JSON with `code`, `message`, `details` fields |
| cursor-opaque | MUST | Conversation cursors are opaque to clients; format is not part of the contract |
| camelcase-json | MUST | All response bodies use camelCase keys |
| sse-resume | MUST | SSE clients can resume from `afterSeq` without replaying history |
| html-sanitized | MUST | Message body HTML is sanitized in Rust before reaching the response |
| secret-redacted | MUST | Secret values are never included in API responses |
| sparse-merge | MUST | PATCH endpoints preserve omitted fields rather than nulling them |
| limit-bounds | MUST | Conversation and message limits are between 1 and 250; invalid values return 400 |
