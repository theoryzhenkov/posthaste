# Frontend / Desktop / MCP / Site Architecture Context

Scope: implementation-derived context for revising the Posthaste SPECial spec around `apps/web`, `apps/desktop`, `apps/mcp`, and `apps/site`. No files were edited except this handoff.

## Executive summary

- `apps/web` is the production mail client UI: React 19 + TypeScript + Vite, TanStack Query for server state, local React/client stores for UI state, and a typed REST/SSE client over `/v1`.
- `apps/desktop` is a Tauri v2 shell around the same web build. Default feature embeds the Rust server in-process and injects `__POSTHASTE_PORT__`/`__POSTHASTE_TOKEN__`; client-only/remote/local-daemon seams exist in the web connection-profile runtime.
- `apps/mcp` is a thin Bun/TypeScript MCP stdio adapter over the daemon OpenAPI contract. It does not implement domain logic; it discovers a daemon and forwards tools to `/v1`.
- `apps/site` is a separate static Astro marketing/download site with a React island. It does not call the daemon and uses Markdown content at build time.
- Current specs (`docs/L0-ui.md`, `docs/L1-ui.md`, `docs/L0-website.md`, `docs/L1-api.md`) cover much of this, but likely spec gaps remain around desktop connection-profile state, MCP architecture/safety, and some implementation mismatches in UI shell/list behavior.

## apps/web: boundary and stack

Relevant files:

- `apps/web/package.json`: web app scripts and dependencies. `build` runs typecheck then Vite; `check` runs logging/api/query-boundary checks, eslint, tsc, prettier. Dependencies include React 19, `@tanstack/react-query`, `@microsoft/fetch-event-source`, Tauri API, shadcn/radix, CodeMirror, dnd-kit, Tailwind.
- `apps/web/src/App.tsx:1-7`: root component identifies itself as QueryClientProvider + toolbar + three-column layout + focused surfaces, with `@spec docs/L1-ui#component-hierarchy` and `docs/L0-ui#navigation-model` annotations.
- `apps/web/src/App.tsx:106-114`: singleton `QueryClient` with default `staleTime: 30_000`, `retry: 1`.
- `apps/web/src/App.tsx:131-200`: `MailClient` owns core shell-local state: `selectedView`, `selectedMessage`, command palette, tag editor, `searchQuery`, shortcut reference, derived prepared search query, theme, account/read-model bootstrap, enabled account gating, mailbox role, and forced settings surface.
- `apps/web/src/App.tsx:520-668`: main shell layout: `ActionBar`, then `ResizablePanelGroup` with `Sidebar`, `MessageList`, conditionally mounted `MessageDetail`; floating/overlay surfaces for command palette, shortcuts, tag editor, compose, invalid route, and `SurfaceHost`.
- `apps/web/src/App.tsx:698-741` (from read): `App` wraps `QueryClientProvider`, `DesignThemeProvider`, `ActiveConnectionProvider`, `DaemonEventBridge`, `ErrorBoundary`, `OperationsProvider`, `ConnectionGate`, and `Toaster`. In Tauri non-main windows with a route surface, it renders `FocusedSurfaceDocument` instead of the full mail shell.

High-level model:

- Web is protocol/storage-thin but interaction-rich. Rust/server owns persistence/JMAP/sync/sanitization; React owns view selection, selected message, search filter, overlays, resizing, keyboard routing, layout persistence, optimistic UI, focused surface routing.
- Initial default view is a smart mailbox `{ kind: 'smart-mailbox', id: 'default-inbox', name: 'Inbox' }` (`App.tsx:116-120`). If no enabled accounts exist, `effectiveView` is `null` and settings can be forced (`App.tsx:162-200`).
- Surface state is serializable and route-backed. Settings/message/attachment/compose surfaces can render as web overlays or Tauri windows. Browser overlay close uses history/back via `closeWebSurface`; desktop close requests are routed through `listenForDesktopCloseRequest` (`App.tsx:201-220`, `desktop.ts`).

## apps/web: API and event usage

Relevant files:

- `apps/web/src/api/client.ts:1-15`: typed HTTP client for `/v1`; Tauri backend port is injected, browser dev falls back to `VITE_API_BASE_URL` or localhost.
- `apps/web/src/api/client.ts:37-59`: base URL/token/Host are dynamic per active connection profile via `getActiveConnection()`, not frozen module constants.
- `apps/web/src/api/client.ts:75-106`: `authHeaders()` sends `Authorization: Bearer` and optional pinned `Host`; comments note SSE/blob fetches share this and no `access_token` query param remains.
- `apps/web/src/api/client.ts:262-360`: settings, read-call, automation preview, account CRUD/OAuth/enable-disable/logo/verify endpoints.
- `apps/web/src/api/client.ts:389-428`: smart mailbox CRUD/reset.
- `apps/web/src/api/client.ts:428-607`: message/conversation/list/search/detail endpoints (`/smart-mailboxes/{id}/messages`, `/smart-mailboxes/{id}/conversations`, `/views/conversations`, `/messages/search`, `/sources/{sourceId}/messages`, `/sources/{sourceId}/messages/{messageId}`).
- `apps/web/src/api/client.ts:607-697`: compose (`identity`, `sender-addresses`, `reply-context`, `commands/send`) and message commands (`set-keywords`, `add/remove/replace-mailbox`, `destroy`) plus sync command.
- `apps/web/src/api/client.ts:714-730`: SSE URL builder for `/events?accountId&afterSeq`.
- `apps/web/src/hooks/useDaemonEvents.ts:1-21`: SSE listener uses `fetchEventSource` instead of native `EventSource` so it can authenticate with headers.
- `apps/web/src/hooks/useDaemonEvents.ts:51-124`: opens stream, resumes from `sessionStorage` key `mail:last-event-seq`, handles fatal 4xx vs retriable 5xx, parses `DomainEvent`, suppresses local echo, applies domain cache updates, and dispatches `mail:domain-event` browser `CustomEvent`.

Important patterns:

- API types are generated from repo-root `openapi.json` into `apps/web/src/api/schema.gen.ts`; `package.json` has `api:generate` and `api:check`.
- Every request logs through `apiLogger` with observability headers and operation context (`api/client.ts:145-220` from read).
- Blob/resource URLs (attachments/logos) are built without token query params and loaded via authenticated fetches where needed.
- SSE events are domain facts; web maps them through `domainCache.applyDomainEvent` and local component listeners. `MessageList` also refetches current list on relevant domain event.

## apps/web: state model and UI shell

Relevant files:

- `apps/web/src/mailboxNavigationReadModels.ts:31-63`: `useMailNavigationReadBootstrap()` sends one typed `POST /read` batch for `Account/list`, `Mailbox/list` (enabled accounts), `SmartMailbox/list`, and `Tag/list`.
- `apps/web/src/mailboxNavigationReadModels.ts:65-92`: read response hydrates normalized React Query caches: accounts, per-account mailboxes, smart mailboxes, tags.
- `apps/web/src/mailboxNavigationReadModels.ts:102-156`: feature code reads the hydrated domain keys and builds sidebar navigation sources; user tags filter out system keywords.
- `apps/web/src/queryKeys.ts`: canonical app-level query keys: settings, accounts, account, sender addresses, compose suggestions, mailboxes, tags, mail navigation read, messages, conversations, smart mailboxes.
- `apps/web/src/mailState.ts:1-83`: owns mail cache key schema (`mailKeys`), `MailSelection`, view selection type, normalized conversation page slices, local echo suppression map.
- `apps/web/src/components/MessageList.tsx:1-18`: message-first middle pane with manual virtualization and live refresh.
- `apps/web/src/components/MessageList.tsx:44-73`: fixed virtualization constants (`ROW_HEIGHT = 30`, `OVERSCAN_ROWS = 6`, page size `100`) and per-view scroll offset cache.
- `apps/web/src/components/MessageList.tsx:116-145`: fetches pages through `messagePageClient.fetchPage()` scoped to smart mailbox or source mailbox, with server query/sort/cursor/limit.
- `apps/web/src/components/MessageList.tsx:200-232`: `useInfiniteQuery` keyed by selected view, search, sort; disabled for blocked client query; pages flatten to display rows after account-name enrichment.
- `apps/web/src/components/MessageList.tsx:270-345`: remembers selected slot and auto-focuses the next message when the selected message leaves the list.
- `apps/web/src/components/MessageList.tsx:348-387`: list-level keyboard shortcuts: `j`/down, `k`/up, `e` archive, `#`/Backspace trash; ignored in editable targets and with modifiers.
- `apps/web/src/components/MessageList.tsx:389-463`: per-view scroll restoration, viewport measurement, domain-event listener, infinite-scroll trigger.
- `apps/web/src/hooks/useEmailActions.ts:1-20`: mail actions are a thin adapter over the operation runner; optimistic cache patching, command dispatch, undoability live in `OperationsProvider`/`operations`.
- `apps/web/src/components/OperationsProvider.tsx:1-18`: operation runner captures before-image, applies optimistic patch, sends commands, rolls back on error, reconciles on success, and maintains undo/redo history.
- `apps/web/src/operations.ts:1-18`: framework-free operation model; mutations are projections over mutable state (`keywords`, `mailboxIds`); undo is generic inversion of captured before/after.

UI shell boundaries:

- `ActionBar` is global chrome and contains command/search entry, compose, reply/action groups, settings/theme/shortcuts toggles.
- `Sidebar` (`apps/web/src/components/Sidebar.tsx`) renders smart mailboxes, tags, accounts and source mailboxes from read models; exposes object-scoped context menu actions.
- `MessageList` is the center pane, message-first (not thread-grouped by default), manually virtualized, server-paginated, sortable/resizable/reorderable columns.
- `MessageDetail` is only mounted when a selected message exists; no selected message means the right pane disappears and list can use space.
- `SurfaceHost` renders full-window focused surfaces; settings/compose use a simpler fixed full-surface overlay while message/attachment include a header with open-in-window and close controls (`apps/web/src/components/SurfaceHost.tsx:1-117`).
- Compose is lazily imported but intentionally eager-ish in the main chunk due Tauri asset-protocol latency (`App.tsx:97-105`).

## apps/web: connection-profile runtime

Relevant files:

- `apps/web/src/connection/types.ts:1-66`: connection profiles support `embedded`, `local-daemon`, and `remote`. Profiles do not carry secrets; remote tokens are in OS keyring; embedded token is injected.
- `apps/web/src/connection/runtime.ts:1-35`: process-wide active connection seeded synchronously to embedded defaults from injection/fallback, preserving old behavior before async resolution.
- `apps/web/src/connection/runtime.ts:51-76`: `applyResolvedConnection()` swaps active connection and notifies subscribers.
- `apps/web/src/connection/resolve.ts:1-25`: resolution mirrors deployment modes and MCP env/daemon-file pattern.
- `apps/web/src/connection/resolve.ts:36-106`: embedded requires injection in Tauri client-only builds, local-daemon reads daemon.json via desktop bridge, remote uses profile base URL + keyring token + optional Host header.
- `apps/web/src/desktop.ts:1-36`: Tauri runtime/window-label detection; main window label is `main`; macOS desktop detection for inset chrome.
- `apps/web/src/desktop.ts:38-90`: desktop bridge commands: toggle devtools, open external URL, open native surface window, browser popup fallback.
- `apps/web/src/desktop.ts:92-134`: close request listener, close current surface window, web surface push/replace/close via hash/history.

Spec implication: connection profiles are partly documented in ephemeral design comments/spec references, but likely need stable SPECial coverage if revising architecture.

## apps/desktop: Tauri shell boundary

Relevant files:

- `apps/desktop/tauri.conf.json`: desktop app product `Posthaste`, identifier `com.posthaste.mail`; frontend dist is `../web/dist`; dev URL is Vite `127.0.0.1:5173`; no static windows in config because windows are built in Rust.
- `apps/desktop/Cargo.toml`: default feature `embedded-server`; `devtools` and Linux-only `e2e-testing` features. Depends on `tauri`, `tauri-plugin-opener`, `posthaste-server`, `posthaste-observability`, `keyring`, `dirs`.
- `apps/desktop/src/lib.rs:272-316`: Tauri commands `open_external_url` and `open_surface_window`; surface windows validate descriptor, compute label/route, reuse existing windows by label, otherwise build `index.html#route` window.
- `apps/desktop/src/lib.rs:324-346`: `BackendInjection` carries embedded backend port/token when feature enabled; no injection in client-only builds.
- `apps/desktop/src/lib.rs:377-412`: `run()` registers Tauri commands: frontend logging, external URL, surface windows, devtools, client connection store/token commands, local-daemon read, plus e2e bridge when feature-enabled.
- `apps/desktop/src/lib.rs:414-460`: setup starts embedded server on `127.0.0.1:0`, adds Tauri/dev loopback CORS origins, manages backend handle, stores auth token, sets menu, creates main window 1200x800.
- `apps/desktop/src/lib.rs:624-663`: window factory injects backend/window label, intercepts external web navigations to system browser, and applies macOS overlay titlebar/traffic-light position.
- `apps/desktop/src/lib.rs:688-720`: initialization script defines `__POSTHASTE_WINDOW_LABEL__`; embedded builds also define `__POSTHASTE_PORT__` and `__POSTHASTE_TOKEN__`.
- `apps/desktop/src/lib.rs:742-855`: surface descriptors map to hash routes, stable labels, titles, and sizes: attachment 1100x820, settings 980x720, message 900x760, compose 780x640. Settings window label is stable `settings`; message/attachment/compose labels are stable hashes.
- `apps/desktop/src/client_connection.rs:1-21`: desktop owns client connection state distinct from daemon roots: `connections.json` and per-profile remote tokens; local-daemon mode reads daemon `STATE_ROOT/daemon.json`.
- `apps/desktop/src/client_connection.rs:42-86`: connection profile store path is Tauri app config dir `client/connections.json`; daemon state root mirrors server/MCP (`POSTHASTE_STATE_ROOT`, else `$XDG_DATA_HOME/posthaste`, else `~/.local/share/posthaste`).
- `apps/desktop/src/client_connection.rs:88-153`: Tauri commands read/write connections JSON, get/set/delete keyring tokens under service `posthaste-client`.
- `apps/desktop/src/client_connection.rs:155-178`: read local daemon port-file and return `{ port, token }`.

Desktop invariants to preserve in spec:

- One web codebase; desktop hosts `apps/web` output.
- Embedded build starts Rust backend in-process and communicates over loopback `/v1` with injected token.
- Surface windows use serializable descriptors + hash routes; content fetches by IDs and does not depend on parent props.
- External links from email/webview are opened via system browser, not in-app navigation.
- Client profile secrets are not stored in `connections.json`; remote tokens use OS keyring.

## apps/mcp: architecture and API usage

Relevant files:

- `apps/mcp/package.json`: Bun TypeScript package with `@modelcontextprotocol/sdk`, `zod`, generated OpenAPI TS types; scripts for `api:generate`, `typecheck`, `build`, `start`.
- `apps/mcp/README.md`: explicitly describes MCP as a thin downstream adapter over `/v1`, not a competing interface; stdio transport; endpoint/token resolved from env or daemon port-file; capability scoping caveat.
- `apps/mcp/src/client.ts:10-20`: connection is `/v1` base URL plus optional token, source is `env` or `daemon.json`.
- `apps/mcp/src/client.ts:64-86`: daemon state root resolution mirrors server: `POSTHASTE_STATE_ROOT`, else `$XDG_DATA_HOME/posthaste`, else `~/.local/share/posthaste`; no macOS Application Support special-case.
- `apps/mcp/src/client.ts:118-151`: resolution order: `POSTHASTE_API_URL`/`POSTHASTE_TOKEN` first, else daemon port-file; env URL must include `/v1`.
- `apps/mcp/src/client.ts:163-229`: `apiFetch()` builds query string, sends bearer token when available, JSON encodes bodies, parses typed API error bodies.
- `apps/mcp/src/index.ts:17-25`: `buildServer()` creates `posthaste-mcp` MCP server with tools capability only.
- `apps/mcp/src/index.ts:27-57`: wrapper converts successful results to JSON text content and API/connection failures to MCP tool errors (`isError`) rather than crashing.
- `apps/mcp/src/index.ts:61-186`: read tools: `list_accounts`, `read_mail_navigation`, `list_conversations`, `get_conversation`, `search_messages`, `get_message`.
- `apps/mcp/src/index.ts:190-260+`: mutating tools: `set_keywords`, `move_to_mailbox`, `send_message` (validated with zod, forwards to message command/send endpoints).

MCP safety/spec gap:

- README says current adapter uses the daemon token with full access, including send; capability scoping is designed but not implemented. This should be prominent in architecture/security specs if MCP is included in stable SPECial docs.
- Tool naming currently exposes `move_to_mailbox` but maps to `add-to-mailbox`, not `replace-mailboxes`; that distinction matters for spec wording.

## apps/site: static site boundary

Relevant files:

- `apps/site/package.json`: Astro 6 + React 19 static site, build/check scripts; dependencies include `gray-matter`, `marked`, Geist fonts, lucide.
- `apps/site/src/pages/index.astro:1-8`: imports fonts, React `App`, `getHomeContent()`, and site CSS; content loaded at build time.
- `apps/site/src/pages/index.astro:23-24`: hydrates `<App content={content} client:load />`.
- `apps/site/src/App.tsx:49-98`: site mock has local state types for inbox/archive, selected message, read/archive/flagged sets, mock theme, landscape time. Persists mock mail UI state under `posthaste-site-mail-mock-state-v1`.
- `apps/site/src/App.tsx:127-180`: landscape phase/celestial position based on local browser time, updated every minute.
- `apps/site/src/App.tsx:193-221`: reveal effect uses IntersectionObserver; respects reduced motion by immediately marking elements visible.
- `apps/site/src/App.tsx:224-243`: React island renders fixed install header, hero mail mock, landscape values, notes, theme, footer.
- `apps/site/src/App.tsx:246-260+`: hero initializes selected mailbox/message and read/archive/flag state from localStorage; it is a mock interaction, not daemon-backed.
- `apps/site/src/content/homeContent.ts:1-19`: imports Markdown content as raw strings from `src/content/home`.
- `apps/site/src/content/homeContent.ts:44-90`: parses Markdown with gray-matter + marked and validates required frontmatter fields.
- `apps/site/src/content/homeContent.ts:92-130`: assembles typed home content from mock emails, notes, open-source section, theme, footer.
- `apps/site/src/content/releasesContent.ts:1-78`: release files are `import.meta.glob`ed from `src/content/releases/*.md`, parsed with frontmatter and marked, validated against known OS values, sorted by version descending.
- `apps/site/src/pages/releases.astro`: loads home footer + release entries at build time, hydrates `Releases` React island.
- `apps/site/src/Releases.tsx`: OS detection happens after hydration via `useSyncExternalStore`; no SSR mismatch; page renders static release download/changelog data.
- `docs/L0-website.md`: already captures many current invariants: site separate from app, no daemon/API calls, Markdown content, static build, Nginx container, releases page source of truth.

Site boundary:

- Site is not the mail client; it is a static showcase/download surface.
- React is used as an interactive island for mock mail shell, install strip, theme/landscape/reveal interactions.
- It intentionally does not import mail client CSS or call local API/JMAP.

## Existing SPECial coverage and likely gaps

Already well covered:

- `docs/L0-ui.md`: thin frontend principle, three-pane shell, layout modes, theme direction, HTML rendering, keyboard, live updates, missing backend UI gaps.
- `docs/L1-ui.md`: component hierarchy, data fetching, MessageList behavior, tags, settings, command search, focused surfaces, desktop window behavior, keyboard shortcuts.
- `docs/L1-api.md`: endpoint table, typed read calls, SSE stream, cursor pagination, compose, message commands.
- `docs/L0-website.md`: site architecture/deployment/releases.

Likely revision gaps from implementation:

1. **MCP needs stable SPECial coverage.** Current README references `docs/eph/DESIGN-L1-mcp-adapter.md`, but no stable L0/L1 MCP spec was found in `docs/`. The revised spec should state MCP is a thin stdio adapter over `/v1`, uses generated OpenAPI types and zod, resolves env/daemon.json, and currently carries full daemon authority.
2. **Connection-profile/client deployment architecture is mostly in comments/eph references.** Web and desktop have concrete `embedded`/`local-daemon`/`remote` profile code, client-owned `connections.json`, keyring token storage, dynamic active connection, Host pinning. This likely belongs in stable API/deployment/frontend architecture docs if SPECial is being revised.
3. **Desktop shell details are implemented and partly in L1 UI, but could be split/clarified.** Tauri owns window construction, backend injection, external navigation interception, profile store commands, menu close behavior, macOS overlay chrome. These are architecture-level, not purely UI visual details.
4. **API docs mention `/config:reload` but web client does not expose it.** `apps/web/src/api/client.ts` has no `config:reload` wrapper. If the spec implies frontend use, mark as backend/API surface not current web UI usage.
5. **`tagsQuery` in `App.tsx` is disabled and resolves empty (`App.tsx:223-227` from read), while tag data is actually hydrated/read in `mailboxNavigationReadModels`; TagEditor receives `knownTags={tagsQuery.data ?? []}` so known tags may be empty there. Spec can avoid saying tag editor always has full known tag set unless verified.
6. **MessageList live update behavior is refetch-on-event, not true live top-of-list merge.** L0 says “React Query invalidation plus list-level merge logic” and live top-of-list inserts preserve viewport; current implementation dispatches events and `MessageList` refetches relevant current view while preserving scroll offset. If revising, either keep aspirational language distinct or align to current implementation.
7. **List is message-first despite some specs/keys still carrying conversation concepts.** L1 notes this, but L0 navigation says conversation list. Implementation `MessageList` rows are `MessageSummary` and selected detail can load conversation context.
8. **Site implementation is static and independent as spec says.** No major gap except release automation/deployment details may be too operational for architecture-level spec depending on revision goals.

## Validation paths for future spec edits

No validation was run beyond code reading. Useful targeted checks if spec edits are made:

- Web: `bun run --cwd apps/web check` or from repo justfile equivalent; specific drift checks include `apps/web/scripts/check-openapi-types.ts`, `check-query-boundaries.ts`, `check-logging-contract.ts`.
- Desktop: Cargo check/test for `apps/desktop` features if spec touches window/connection behavior.
- MCP: `bun run --cwd apps/mcp typecheck` and `bun run --cwd apps/mcp api:generate` if OpenAPI contract changed.
- Site: `bun run --cwd apps/site check` and `bun run --cwd apps/site build` for site spec/implementation changes.

## High-value source list

- `apps/web/src/App.tsx` — shell composition, providers, local state, focused-surface routing, connection gate.
- `apps/web/src/api/client.ts` — REST client, auth headers, endpoint wrappers, SSE URL.
- `apps/web/src/hooks/useDaemonEvents.ts` — authenticated SSE, replay cursor, local echo suppression, cache/event dispatch.
- `apps/web/src/mailboxNavigationReadModels.ts` — typed read-call bootstrap and cache hydration.
- `apps/web/src/mailState.ts`, `apps/web/src/components/OperationsProvider.tsx`, `apps/web/src/operations.ts` — cache schema, optimism, undo/redo.
- `apps/web/src/components/MessageList.tsx` — server-paginated virtualized list and live-event behavior.
- `apps/web/src/connection/*`, `apps/web/src/desktop.ts` — connection profiles and Tauri/browser bridge.
- `apps/desktop/src/lib.rs`, `apps/desktop/src/client_connection.rs`, `apps/desktop/tauri.conf.json` — desktop shell, embedded backend, windows, profile store/keyring/local-daemon discovery.
- `apps/mcp/src/index.ts`, `apps/mcp/src/client.ts`, `apps/mcp/README.md` — MCP adapter tools, daemon discovery, error handling, safety caveat.
- `apps/site/src/pages/*.astro`, `apps/site/src/App.tsx`, `apps/site/src/content/*Content.ts`, `apps/site/src/Releases.tsx` — static site/content/release architecture.
- Specs to compare/update: `docs/L0-ui.md`, `docs/L1-ui.md`, `docs/L1-api.md`, `docs/L0-website.md`; likely add/update stable MCP/deployment architecture docs.
