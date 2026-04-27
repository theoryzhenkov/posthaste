---
scope: L1
summary: "REST endpoint contracts, request/response schemas, error codes, SSE event stream"
modified: 2026-04-27
reviewed: 2026-04-27
depends:
  - path: docs/L0-api
  - path: docs/L0-providers
  - path: docs/L1-sync
  - path: docs/L1-jmap
dependents:
  - path: docs/L1-ui
---

# API domain -- L1

## Endpoint table

All endpoints are prefixed with `/v1`.

In browser-localhost mode, `posthaste serve` serves the built React frontend on non-API paths and keeps all JSON/SSE endpoints under `/v1`. Unknown `/v1` paths return API 404s rather than the frontend shell.

### Settings

| Method | Path | Handler | Request | Response |
|--------|------|---------|---------|----------|
| GET | `/settings` | `get_settings` | -- | `AppSettings` |
| PATCH | `/settings` | `patch_settings` | `PatchSettingsRequest` | `AppSettings` |
| POST | `/automation-rules:preview` | `preview_automation_rule` | `PreviewAutomationRuleRequest` | `AutomationRulePreviewResponse` |

### Accounts

| Method | Path | Handler | Request | Response |
|--------|------|---------|---------|----------|
| GET | `/accounts` | `list_accounts` | -- | `AccountOverview[]` |
| POST | `/accounts` | `create_account` | `CreateAccountRequest` | `AccountOverview` |
| GET | `/accounts/{account_id}` | `get_account` | -- | `AccountOverview` |
| PATCH | `/accounts/{account_id}` | `patch_account` | `PatchAccountRequest` | `AccountOverview` |
| DELETE | `/accounts/{account_id}` | `delete_account` | -- | `OkResponse` |
| POST | `/accounts/{account_id}/verify` | `verify_account` | -- | `VerificationResponse` |
| POST | `/oauth/start` | `start_provider_oauth` | `StartProviderOAuthRequest` | `StartOAuthResponse` |
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
| GET | `/sender-addresses` | `list_sender_addresses` | -- | `CachedSenderAddress[]` |
| GET | `/sources/{source_id}/identity` | `get_identity` | -- | `Identity` |
| GET | `/sources/{source_id}/messages/{id}/reply-context` | `get_reply_context` | -- | `ReplyContext` |
| POST | `/sources/{source_id}/commands/send` | `send_message` | `SendMessageRequest` | `OkResponse` |

`SendMessageRequest` includes optional `from: Recipient`. When present, the
backend uses that sender address for the outgoing RFC 5322 `From` field. The
route `source_id` is still the account that submits the message; the frontend
may choose that account from configured sender suggestions or wildcard ownership.
After a successful send, the backend records the selected sender in the local
SQLite `sender_address_cache`; failed or rejected sends do not update it.
`GET /sender-addresses` returns those accepted free-form sender suggestions
across accounts for compose autosuggest.

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

**Create**: `POST /accounts` accepts account name, optional full name, email address/pattern ownership, provider driver, transport details, and a secret instruction. If `id` is omitted, the backend derives an internal unique ID from the first email pattern or account name. The endpoint applies the secret instruction, validates required fields, persists to config, starts the supervisor runtime, and emits an `account.created` event. JMAP accounts require a base URL and configured secret; username is optional for bearer-token auth. IMAP/SMTP accounts require username, configured secret, explicit IMAP and SMTP endpoints, and a concrete sender address via `emailPatterns`.

**Patch**: `PATCH /accounts/{id}` merges provided fields into the existing account. Omitted fields in the transport sub-object preserve their current values (sparse merge). Secret handling uses the backend `SecretWriteMode` tri-state: `keep` (preserve existing), `replace` (store new secret in keyring), `clear` (delete managed secret). The settings UI exposes this as an empty password field to keep the configured secret or a filled password field to replace it.

**Delete**: `DELETE /accounts/{id}` removes the managed OS keyring secret (if any), treating an already-missing keyring entry as deleted, stops the supervisor runtime, deletes the config file, and emits an `account.deleted` event.

**Verify**: `POST /accounts/{id}/verify` attempts provider connection setup and returns whether the connection succeeded, the primary identity email when available, and whether push is supported.

**OAuth**: `POST /oauth/start` starts provider-first OAuth setup for a built-in provider. `POST /accounts/{id}/oauth/start` starts the same authorization-code flow for an existing account whose provider has a built-in profile. The request supplies OAuth `clientId`, optional `clientSecret` for providers such as Google Desktop OAuth that require it at the token endpoint, and loopback `redirectUri`; the backend stores the PKCE verifier and OIDC nonce, then returns only the authorization URL, state, and redirect URI. `GET /oauth/callback` validates the one-time state, exchanges the authorization code, discovers and caches the provider JWKS, verifies the ID-token signature, checks the issuer, audience, expiry, nonce, and verified-email status, stores the token set as an OS-keyring secret, and either creates a new IMAP/SMTP account from the provider identity email or updates the existing account secret, switches the existing transport to `auth: oauth2`, restarts the supervisor runtime, and emits the matching `account.created` or `account.updated` event.

**Enable/Disable**: Toggle `enabled` flag, re-persist, and restart the supervisor (which respects the flag).

**Transport**: Account transport JSON uses camelCase. Common fields are `provider`, `auth`, `username`, `secret`, and optional JMAP `baseUrl`. IMAP/SMTP accounts also include `imap` and `smtp` endpoint objects with `host`, `port`, and `security` (`tls`, `startTls`, or `plain`). `PATCH /accounts/{id}` sparse-merges the transport object and preserves omitted sub-fields.

**Appearance**: `AccountOverview` includes a resolved `appearance` object for the account mark. Account config may persist either `{ kind: "initials", initials, colorHue }` or `{ kind: "image", imageId, initials, colorHue }`. If no appearance is configured, the API derives initials and a stable hue from the account. `PATCH /accounts/{id}` can update letter/color appearance. `POST /accounts/{id}/logo` accepts raw PNG, JPEG, WebP, or GIF bytes up to 2 MiB, stores the image under the config root, updates account appearance to `image`, and returns the updated overview. Logo bytes are served from `GET /account-assets/logos/{image_id}`.

**Automation rules**: `AppSettings` and `PatchSettingsRequest` include `automationRules` for active rules and `automationDrafts` for persisted incomplete editor state. Each rule has `id`, `name`, `enabled`, `triggers`, `condition`, `actions`, and `backfill`. `condition` uses the same smart-mailbox rule tree as saved searches. Account and mailbox restrictions are ordinary query conditions, not a separate rule scope. PATCH replaces the full active rule list when `automationRules` is present and preserves it when omitted; the same replacement rule applies to `automationDrafts`. Active rule IDs must be unique, active rules need at least one trigger and one action, tag actions must target non-system keywords, and move actions must target a non-empty mailbox ID. Draft rule IDs must be present and unique across active and draft rules, but draft names, triggers, and actions may be incomplete. Draft rules are not executed and do not enqueue backfill. When `automationRules` is present, the backend saves the rules and enqueues durable low-priority backfill jobs for enabled accounts if the current enabled backfill-rule fingerprint has not already completed.

`POST /automation-rules:preview` accepts a draft rule `condition` and optional `limit`, then returns `AutomationRulePreviewResponse { total, items }` using the same indexed rule evaluator as smart mailboxes. Results are newest-first. The default preview limit is 5 and the maximum is 50.

The frontend exposes mailbox action editors, but they persist through global `automationRules` and `automationDrafts`. Pressing Save action writes the current editor item as an active rule when it passes active-rule validation, or as a draft otherwise. A smart-mailbox action is represented as a global automation with an ID prefix owned by that smart mailbox and a condition combining the selected account condition, the smart-mailbox rule, and the action rule's own condition. A source-mailbox action is represented as a global automation with an ID prefix owned by the source mailbox and a condition combining the selected account condition, the source mailbox ID condition, and the action rule's own condition.

## Secret management

Account secrets are opaque authentication material. For JMAP accounts this may be an OAuth token set, a provider API token, or a development credential accepted by the provider. For OAuth accounts, the stored OS-keyring value is a JSON token set containing the access token, refresh token when granted, expiry, scopes, provider, and OAuth client ID; the runtime refreshes it before provider connection and passes only the current access token to XOAUTH2-capable gateways. The API must not assume that the value is a Fastmail app-specific password.

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
