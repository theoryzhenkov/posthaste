---
title: "PLAN — Legacy web client port onto the integrated backend"
scope: L2
summary: "Port inventory for moving legacy/web onto apps/client (POST /query, POST /command, GET /events, blobs). Four buckets over ~60.8k LOC of TS/TSX: ~8.7k PORTS-AS-IS, ~21k REWIRE, ~19k DIES (seam: runtime link/wasm replica/domain-cache/connection/old schema.gen), ~11.7k BLOCKED on missing API exposure (settings r/w, smart mailboxes, account management+OAuth, mailbox CRUD/roles, tags, rules, snooze, undo/revlog, op retry/cancel, sync-now, unsubscribe, sender-addresses). Domain service implements nearly all blocked capabilities server-side; the gap is exposure through the new Query/Command enums. react-query is KEPT — generation-advance global invalidation replaces per-key invalidation."
modified: 2026-07-17
lifecycle: ephemeral
type: PLAN
state: inventory-complete
depends: [../architecture/L1-architecture, ../api/L1-api, ../client/L1-client]
dependents: []
---

# PLAN — Legacy web client port

Ground truth used: `legacy/web/src` at HEAD (60,765 LOC .ts/.tsx, + 515 LOC css);
new surface = `apps/client/models/src/{query,command,event}.rs` / generated
`apps/client/frontend/src/gen/`; backend routes in
`apps/client/backend/src/api.rs` (`/query`, `/command`, `/events`,
`/blobs/{blob_id}` — nothing else); server-side capability check against
`crates/posthaste-domain-service/src/service/*`.

New query families: `mailList`, `thread`, `messageDetail`, `mailboxCounts`,
`accounts`, `pendingOperations`.
New commands: `setKeywords`, `replaceMailboxes`, `destroy`, `createDraft`,
`updateDraft`, `discardDraft`, `send` (hold: `sendAt`/`undoWindowSeconds`),
`createAccount`/`updateAccount` (identity-only).

LOC figures are `wc -l` per directory (recursive), tests included; bucket
totals are approximate where a directory splits.

## 1. PORTS-AS-IS (~8.7k LOC)

Pure presentation, styles, editors, parsers, local-preference and desktop
glue. Only import paths change.

| path | loc | notes |
|---|---|---|
| components/ui/ | 1,278 | shadcn/radix primitives |
| components/floating-panel/ | 1,003 | panel chrome; geometry lib below |
| components/keyboard/ | 885 | key handling, chord display |
| design/ | 743 | theme/tokens/density/glass |
| query-language/ | 649 | pure tokenizer/validator/completions — but grammar must be re-aimed at `MailListQuery` filters (see §5.B); vocabulary data in `queryDefinitions.ts` (221) |
| surfaces/ | 593 | surface descriptors/routing (serialize/parse) |
| client-preferences/ | 541 | localStorage + BroadcastChannel appearance prefs |
| floating-panel-geometry/ | 499 | pure geometry |
| onboarding/ | 460 | tour UI; add-account step blocked on §5.C |
| components/markdown-composer/ | 145 | codemirror wrapper |
| utils/, lib/, config/ | 149 | relativeTime, cn(), oauthProviders.json (data used by blocked flow) |
| root utilities | ~2,300 | layering, floatingPanelLayout+Geometry, markdownEditing, themeSettings, brandFavicon, searchQuery, surfaceHistory/Window*, composeIntent, logger, consoleCapture, observability, logEvents, desktop.ts/desktopUpdates/desktopRepair/developerTools/diagnostics (tauri-side), progressValue |
| index.css + assets | 515 | tailwind theme |

## 2. REWIRE (~21k LOC)

Keeps its UI; its data access swaps to facade hooks/verbs. react-query stays;
`queryKeys.ts`-style granular invalidation is deleted in favor of
generation-advance global invalidation.

| path | loc | target family / verb | notes |
|---|---|---|---|
| components/ (top level) | 4,842 | mixed | MessageList/MessageRow → `mailList`; MessageDetail/EmailFrame → `messageDetail` (bodyHtml/bodyText inline now; cid images → `/blobs`); ActionBar → `setKeywords`/`replaceMailboxes`; AttachmentSurface → `/blobs` + `MessageAttachment` meta; ComposeOverlay shell → draft commands; SettingsPanel shell → §5; snooze entries blocked (§5.G) |
| components/compose-overlay/ | 2,329 | `createDraft`/`updateDraft`/`discardDraft`/`send` | reply headers derivable client-side (`MessageSummary.rfcMessageId`/`inReplyTo` + `messageDetail` body for quoting) replacing `/reply-context` + `/draft-content`; address autocomplete blocked (§5.K); undo-send = `send.undoWindowSeconds` |
| components/sidebar/ | 1,919 | `mailboxCounts`, `accounts` | smart-mailbox rows blocked (§5.B); create/delete-mailbox mutations blocked (§5.D); reorder/colors blocked on settings (§5.A) |
| components/message-list/ | 1,620 | `mailList` (windowed limit+cursor) | `useRuntimeMailListView.ts` (~310) DIES — runtime-view seam replaced by facade live query |
| command-search/ | 1,918 | `mailList` (freeText), `mailboxCounts`, `accounts` | providers/messages|mailboxes|tags feed from queries; tags provider blocked (§5.E) |
| actions/ | 1,219 | command verbs | registry stays; defs/message.ts dispatch → `setKeywords`/`replaceMailboxes`/`destroy`; snooze defs blocked (§5.G) |
| app/ | 1,073 | facade | MailClient.tsx becomes provider of new `MailClient`; queryClient.ts keeps react-query with global invalidation |
| hooks/ (rewire portion) | ~1,000 | | useEmailActions → command verbs; useAccountsView → `accounts`; useDockBadge → `mailboxCounts`; useAutoMarkRead → `setKeywords`; useMessageBody → `messageDetail`; useMailboxColors/Role, useTagAppearance blocked (§5.A/D/E) |
| components/thread-list/ | 899 | `thread` | |
| components/message-detail/ | 800 | `messageDetail` | snoozePresets.ts blocked (§5.G); unsubscribe action blocked (§5.J) — `ListUnsubscribe` metadata already in gen/ |
| notifications/ | 579 | `/events` prompts | newMailArrivals gate keys on `message.updated` payload flags (`arrived`/`created`) — new `DomainEventPayload.payload` is verbatim pass-through, verify flags survive |
| components/command-palette/ | 409 | as command-search | |
| components/tags/ | 227 | keywords via `setKeywords` | tag list/appearance blocked (§5.E/A) |
| root read-models | ~1,100 | `accounts`, `mailboxCounts` | accountHealth.ts (328) → `AccountRow.status/push/lastSync*`; mailboxNavigationReadModels.ts (263) — smart-mailbox/tags portions blocked; mailboxRoles.tsx (143); accountDirectory.ts; composeMessage.ts; messageBody.ts (32, already shaped for bodyHtml/bodyText); attachments.ts |
| api/types/ | 1,171 | gen/ | hand-written wire types: mail/read/compose replaced by gen/ twins; appearance/settings/notifications/rules types survive as local types until §5.A/F land |

## 3. DIES (~19k LOC)

The seam. Deleted, not ported.

| path | loc | what it was |
|---|---|---|
| runtime/ | 7,445 (+3.9k wasm binary lines) | runtime link client, adapters (http/fake), mutations catalog, view frames, nearEnd, replica/ (wasm entityStoreAdapter, worker store port, pendingSetStore, undoHistoryStore, countOverlay, replicaDatabase/IDB durability), wasm/ bindings | 
| api/ (minus api/types) | 8,199 | schema.gen.ts (6,365, old OpenAPI), events.gen.ts, querySchema.gen.ts, api/client/* (per-endpoint fetch wrappers incl. runtimeLinks.ts), conformance/ | 
| connection/ | 1,213 | profile/connection store, resolve, ConnectionScreen, injected — replaced by facade connection-info discovery |
| domain-cache/ | 856 | domain-event → react-query cache surgery (handlers, invalidations, mailboxCounts overlay, resources) — replaced by global invalidation |
| mail-state/ | 198 | cache write-through lookup helpers |
| live-store/ | 142 | main-thread mirror of wasm replica (D115) |
| hooks (seam portion) | ~700 | useDaemonEvents (facade owns events), useRevLogMirror + RevLogMirrors, useUndoRedo (replica applyDiff stack — see §5.H), useReplicaDatabaseReloadPrompt, useRuntimeResourceObjectUrl |
| root glue | ~170 | domainCache.ts, messagePageClient.ts, mailState.ts, queryKeys.ts (granular keys), surfaceBootstrapLog.ts (partly) |
| scripts/ checks | — | check-openapi-types, gen/check-event-topics, gen/check-query-schema, check-runtime-boundaries, check-query-boundaries |

Also superseded: `apps/client/frontend` skeleton UI (`src/app/`) — `client.ts`,
`hooks.tsx`, `gen/` are the transport core the port builds on.

## 4. BLOCKED (~11.7k LOC)

| path | loc | missing surface |
|---|---|---|
| components/settings-panel/ | 10,807 | nearly every pane: accounts-pane/account-editor (§5.C), smart-mailboxes-pane + SmartMailboxEditor (§5.B), AutomationsPane/automation-actions/rule-group (§5.F), GeneralPane/AppearancePane/NotificationsPane (§5.A), TagsPane + tag mutations (§5.E), MailboxSelect/SourceMailboxEditor role+color (§5.D/A), TroubleshootingPane/SyncProgressMeter (§5.I), StoragePane, UpdatesSection (tauri, ports); OutboxPane (~250) is NOT blocked — rewires to `pendingOperations` today, retry/cancel actions blocked (§5.L) |
| automation-rules/ | 381 | §5.F rules CRUD + preview |
| composeAddressSuggestions.ts | 175 | §5.K sender-addresses |
| hooks/useTagAppearance, useMailboxColors, useMailboxRole | ~300 | §5.A settings write, §5.D role |
| automationRules.ts (root) | 32 | §5.F |
| scattered: snooze UI in ActionBar/CommandPalette/MessageDetail/actions | ~400 (counted in §2 rows) | §5.G snooze |

## 5. Missing backend surface, ranked by blocked UI

All checked against `crates/posthaste-domain-service`. **Server-side capability
exists in almost every case; the gap is exposure through the new Query/Command
enums in `apps/client/models` + handlers in `apps/client/backend/src/api.rs`.**

| # | surface | needed queries/commands | server-side today | blocks |
|---|---|---|---|---|
| A | Settings read/write | `appSettings` query + `updateSettings` command (appearance, notification prefs, undo-send default, mailbox colors, tag appearance) | `config_delegates.rs: get_app_settings/put_app_settings` ✔ | ~3.5k: General/Appearance/Notifications panes, theme sync, mailbox colors, tag appearance |
| B | Smart mailboxes | `smartMailboxes` list query + create/update/delete/reset-defaults commands + `mailList` accepting a smart-mailbox scope (rule) | `smart_mailbox_queries.rs` (list/find/query_message_page_by_rule/conversations) + `config_delegates.rs` CRUD ✔ | ~2.5k: sidebar nav rows, smart-mailboxes-pane, editor, command-search provider, query-language views |
| C | Account management beyond identity | transport+secret settings read/write, verify, enable/disable, delete, logo upload + logo asset GET, OAuth start/callback | wizard + `posthaste-server/src/oauth_routes` ✔ (needs re-homing to apps/client/backend), `config_delegates.rs` insert/save/delete_source ✔ | ~3k: accounts-pane, account-editor, onboarding add-account |
| D | Mailbox CRUD + roles | createMailbox/deleteMailbox/renameMailbox commands, setMailboxRole | `mailbox_queries.rs: create_mailbox/destroy_mailbox/set_mailbox_role` ✔ | ~500: sidebar create/delete, role mutation |
| E | Tags enumeration | `tags` query (per-account + merged) | `mailbox_queries.rs: list_tags/list_merged_tags` ✔ | ~800: TagsPane, tag pickers, command-search tags provider |
| F | Automation rules | rules list/create/update/delete + `automation-rules:preview` equivalent | `service/automation/` (apply/backfill/jobs) ✔ | ~1.5k: AutomationsPane, automation-rules/ |
| G | Snooze / unsnooze | snooze(messageId, until) + unsnooze commands (auto-return already runs server-side) | `store/snooze.rs`, `mutation.rs: auto_return_snoozed_messages` ✔ | ~400: snooze presets, ActionBar/palette/actions entries, snooze mailbox role |
| H | Undo/redo (mutation history) | undo/redo command + revlog/history query (per DESIGN-L2-undo-redo-revlog-contract) | `store/rev_log.rs` ✔; NS2 "one-intent undo" covers send-hold only | ~700: useUndoRedo, RevLogMirrors, toast Undo — port can ship v1 without Ctrl+Z |
| I | Sync-now | syncAccount command (mode) | `sync_ops.rs: sync_account_with_mode` ✔ | ~300: Troubleshooting refresh, SyncProgressMeter |
| J | Unsubscribe | unsubscribe command (one-click POST path; mailto: path works via compose) | legacy endpoint existed; `ListUnsubscribe` metadata already on `messageDetail` ✔ | ~100: MessageDetail unsubscribe button |
| K | Sender addresses | `senderAddresses` query (autocomplete corpus) | legacy `/sender-addresses` cache | ~200: compose address suggestions |
| L | Operation retry/cancel | retryOperation / cancelOperation commands (`pendingOperations` read exists) | outbox modules ✔ | ~150: OutboxPane row actions |
| M | Message raw/source + body formats | raw/source variant on `messageDetail` or blob of full RFC822 | legacy `/body?format=` | small; reader uses inline bodyHtml/bodyText already |
| N | Attachment download naming | `Content-Disposition: attachment; filename=` on `/blobs/{id}` (filename already in `MessageAttachment`) | trivial | download UX |
| O | Auth tokens | token minting (`/auth/tokens` equivalent) | specified in L1-api §6, staged | MCP/ctl migration + UI token pane, not the v1 port |
| P | Diagnostics | health/config-reload equivalents | `reload_config` ✔ | Troubleshooting extras |

## 6. Old /v1 endpoint checklist (legacy/web call sites)

From `api/client/*` + `schema.gen.ts`. Mapping: ✔ = covered by new surface,
§X = blocked item above, seam = dies.

- `/views/conversations`, `/views/conversations/{id}` → ✔ `mailList` (conversation mode) / `thread`
- `/sources/{s}/messages`, `/messages/{m}`, `/messages/search` → ✔ `mailList` (+freeText), `messageDetail`
- `/sources/{s}/messages/{m}/body`, `/attachments/{a}` → ✔ `messageDetail` inline + `/blobs` (§5.M/N residue)
- commands `set-keywords`, `add-to-mailbox`, `remove-from-mailbox`, `replace-mailboxes`, `destroy` → ✔ `setKeywords`/`replaceMailboxes`/`destroy`
- `commands/unsubscribe` → §5.J
- `commands/send`, `save-draft`, `delete-draft` → ✔ `send`/`createDraft`+`updateDraft`/`discardDraft`
- `/sources/{s}/identity`, `/sender-addresses`, `/reply-context`, `/draft-content` → ✔ `accounts` + `messageSummary` headers; §5.K residue
- `/sources/{s}/operations` (+ delete/retry) → ✔ `pendingOperations`; §5.L residue
- `/sources/{s}/mailboxes` (GET/POST/DELETE/PATCH) → ✔ read via `mailboxCounts`; write §5.D
- `/smart-mailboxes*` (list/get/patch/delete/reset, /messages, /conversations) → §5.B
- `/accounts*` (CRUD, logo, verify, enable, disable, oauth/start), `/oauth/start`, `/oauth/callback`, `/account-assets/logos/{id}` → identity ✔ (`createAccount`/`updateAccount`); rest §5.C
- `/settings` (GET/PATCH), `/read` (batch) → §5.A; batch read replaced by parallel queries
- `/rules*`, `/automation-rules:preview` → §5.F
- `/commands/sync` → §5.I
- `/events` → ✔ `GET /events` (topics preserved as `DomainEventPayload.kind` strings)
- `/runtime/sessions*` (5 paths + stream) → seam, dies
- `/auth/tokens` → §5.O; `/health`, `/config:reload` → §5.P

## 7. Dependency delta (legacy/web/package.json → ported app)

KEEP (same majors): react 19, react-dom 19, `@tanstack/react-query` ^5 (kept;
global invalidation), radix-ui ^1.4, shadcn ^4, cmdk, lucide-react,
class-variance-authority, clsx, tailwind-merge, tailwindcss ^4 +
`@tailwindcss/vite`, tw-animate-css, sonner ^2, `@dnd-kit/*`,
react-resizable-panels ^4, `@codemirror/*` + `@lezer/highlight`,
`@fontsource-variable/geist(-mono)` + geist, `@tauri-apps/api` ^2 + plugins
(notification/process/updater), pino (logging contract).

DIES with the seam: `@microsoft/fetch-event-source` (new facade uses native
`EventSource` with `?token=`), devDeps `openapi-typescript` (gen/ comes from
ts-rs), `vite-plugin-wasm`, `fake-indexeddb`; scripts api:generate /
events:generate / query-schema:generate and the runtime/query boundary checks.

Target `apps/client/frontend/package.json` today has only react/react-dom —
the port carries the KEEP list over.

## 8. apps/mcp + posthastectl (migrate LATER — the eventual gap)

One TS package: MCP server + `posthastectl` bin (`src/cli.ts`), sharing
`src/client.ts` over the old `/v1`. Actual call sites:

- reads: `/accounts`, `/read` (batch: Account/Mailbox/SmartMailbox/Tag list), `/messages/search`, `/views/conversations(/{id})`, `/sources/{s}/mailboxes(/{m})`, `/sources/{s}/messages/{m}`, `/messages/{m}/reply-context`
- writes: `commands/set-keywords`, `add-to-mailbox`, `replace-mailboxes`, `destroy`/`unsubscribe` (kind-parametrized), `commands/send`, `commands/sync`
- infra: `/auth/tokens`, `/events` (watch/hook), `/hook`
- deps: `@modelcontextprotocol/sdk`, `zod`

Gap at migration time = §5.B (smart-mailbox list), §5.E (tags), §5.I (sync),
§5.J (unsubscribe), §5.O (tokens), plus batch-read convenience (replaceable by
parallel queries) and `/hook`.

## 9. Proposed port order

1. **Foundation**: carry `client.ts` facade + `gen/` into the ported app; port PORTS-AS-IS bucket wholesale (design, ui, keyboard, panels, surfaces, prefs, utilities); stand up app shell (app/) on the facade with react-query global invalidation.
2. **Mail core (no new backend surface)**: message-list/thread-list/message-detail on `mailList`/`thread`/`messageDetail`; useEmailActions on `setKeywords`/`replaceMailboxes`/`destroy`; sidebar on `mailboxCounts`+`accounts` (smart-mailbox rows behind a flag); notifications on `/events`; OutboxPane on `pendingOperations`. Delete the seam (bucket 3) in the same stroke — the boundary checks go with it.
3. **Compose**: compose-overlay on draft commands + `send` (undo-send via hold); client-side reply-context derivation; attachments via base64 in `send`.
4. **Backend surface wave 1** (unblocks most LOC): §5.A settings r/w → §5.B smart mailboxes → §5.D mailbox CRUD/roles → §5.E tags. Port the corresponding sidebar/settings panes as each lands.
5. **Backend surface wave 2**: §5.C account management + OAuth (accounts-pane, onboarding), §5.F rules, §5.G snooze, §5.I sync-now, §5.J–N small residues.
6. **Later**: §5.H undo/redo surface (ship v1 without Ctrl+Z; undo-send already works), §5.O tokens, then the apps/mcp + posthastectl migration (§8).
