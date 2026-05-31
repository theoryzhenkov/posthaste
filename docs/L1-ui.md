---
scope: L1
summary: "React component hierarchy, visual contract boundaries, list behavior, live updates, HTML rendering"
modified: 2026-05-31
reviewed: 2026-05-31
depends:
  - path: docs/L0-ui
  - path: docs/L0-testing
  - path: docs/L0-branding
  - path: docs/L0-api
  - path: docs/L1-sync
  - path: docs/L1-search
  - path: docs/L1-compose
dependents:
  - path: docs/L2-ui-visual-reference
  - path: docs/L1-lab
---

# UI Domain -- L1

## Component hierarchy

```
App
├── QueryClientProvider
└── MailClient
    ├── ActionBar
    │   ├── Traffic lights
    │   ├── Compose chip
    │   ├── Reply group
    │   ├── Message action group
    │   ├── QuerySearch / command palette trigger
    │   ├── Shortcut trigger
    │   ├── Settings trigger
    │   └── Theme trigger
    ├── ResizablePanelGroup (shell)
        ├── Sidebar
        │   ├── Quick filters
        │   ├── Smart mailbox section
        │   ├── Tags section
        │   └── Account mailbox sections
        └── ResizablePanelGroup (mail content)
            ├── MessageList
            │   ├── Column header bar (SortableColumnHeader + ColumnResizeHandle)
            │   ├── Message query for the selected mailbox or smart mailbox
            │   ├── Virtualized visible rows
            │   └── Live refresh hook
            └── MessageDetail (mounted only while a message is selected)
                ├── Metadata header
                ├── Tag strip
                ├── Attachment strip
                └── EmailFrame or text fallback
    ├── FloatingPanel
    │   ├── CommandPalette
    │   ├── ShortcutReference
    │   └── Compose
    ├── SurfaceHost
    │   └── FocusedSurface
    │       ├── MessageDetail
    │       ├── AttachmentSurface
    │       ├── SettingsPanel
    │       └── Compose
```

The exact visual contract for these surfaces lives in [L2-ui-visual-reference](L2-ui-visual-reference.md). L1 owns interaction and data rules; L2 owns dimensions, colors, typography, and visual states.

## Data fetching

React Query manages server state, but different surfaces use different strategies:

- `queryKeys.accounts` loads configured account overviews.
- `queryKeys.mailboxes(accountId)` loads synced mailboxes for account-level settings.
- `queryKeys.sidebar` loads enabled sources, smart mailbox summaries, and tag summaries.
- `queryKeys.messages(selectedView, query, sort)` loads paginated individual message summaries for the selected mailbox or smart mailbox, with filtering and sorting executed by the backend.
- `mailKeys.conversation(conversationId)` loads the selected conversation's message summaries.
- `mailKeys.message(sourceId, messageId)` loads full message detail, including lazily fetched body content when needed.
- Attachment focused surfaces reuse `mailKeys.message(sourceId, messageId)` and resolve the selected attachment by `attachmentId`; they must not depend on parent-only React props.

Mutable account display fields are canonical in the accounts query. Message and sidebar DTOs may contain `sourceName` snapshots, but the UI resolves visible account names from `sourceId -> account.name` through the account directory selector.

Domain events and mutation results update caches through the centralized domain cache helper. Components should not invent ad hoc cache keys or scatter account/message invalidation rules locally. The message list still listens for live domain events and refreshes the current view when a relevant message or mailbox event arrives.

## Loading And Progress

Long-running loads and process state use the shared `ProgressBar` primitive rather than one-off pulse bars. It supports determinate percentages when the backend exposes counts and indeterminate motion when only pending state is known. Message detail loads, uncached body fetches, attachment preview loads, and account sync progress all use the same primitive so future tasks can share labels, accessibility, and reduced-motion behavior.

The progress bar supplements skeleton layout placeholders; it does not replace the component's spatial placeholder. For an uncached message, the reader shows a compact progress meter above the header skeleton while the full detail is fetched. For an attachment focused surface, the shell shows progress while metadata loads and the preview area shows progress until the image or iframe finishes loading.

## MessageList

`MessageList` is message-first and currently does manual fixed-row virtualization rather than depending on a virtualization library.

- Row height follows the visual density contract: `24px` compact, `30px` standard, `48px` roomy.
- Rows represent individual `MessageSummary` records, not grouped threads.
- The visible slice is derived from `scrollTop`, `viewportHeight`, and overscan rows.
- Scroll offset is preserved per selected mailbox or smart-mailbox key.
- The active command/search filter, sort field, and sort direction are sent to the message endpoint and executed server-side. The frontend virtualizes loaded pages and fetches the next page near the viewport end.
- Empty list space or `Escape` clears the selected message. When no message is selected, the detail pane is closed so the message list can use the available width.
- `j`/`ArrowDown` and `k`/`ArrowUp` move the selection.
- When the selected message leaves the list (archived, trashed, moved, or removed by any refresh), the next message is auto-focused: the row that shifted up into the former slot becomes selected, so the reader advances without manual navigation. Archiving the last row falls back to the new last row; emptying the list clears the selection and closes the detail pane. This is scoped to the current view, so a selection carried in from another mailbox/sort/search does not trigger a jump. Subsequent `j`/`k` continue from the auto-focused message (and still resume relative to the former slot if the selection is ever absent).
- The sidebar is resized in a separate shell panel group from the message list and detail pane, so selecting or deselecting a message does not change the left pane width.
- Thread viewing is not the default list mode. When the user wants a thread, a command may apply a thread filter to the message list.

Each row represents one message. The standard density row is tabular, not card-like. It displays unread state, flag state, attachment state, subject, sender, date, account, and tags according to the L2 column contract.

Message rows expose primary message actions through a right-click context menu. Opening the context menu selects the row first so command targets stay explicit. The menu is built from a contextual-action layer (`actions/contextualActions.ts`) rather than a fixed list, so availability and labels follow the current view's mailbox role: open, mark read/unread, and flag/unflag are always present; **Archive** appears except in archive/trash; **Move to Inbox** (restore) appears in trash/archive/junk; **Move to Trash** appears except in trash; and **Delete permanently** appears in trash. Smart-mailbox and search views are role-ambiguous and fall back to the inbox-like set (archive + move to Trash). This is the first slice of a broader contextual / user-defined action registry (`docs/eph/PLAN-L1-contextual-actions.md`).

## Tags

Tags are user-facing non-system JMAP keywords. The sidebar Tags section is
derived from synced message keywords and selecting a tag applies a `tag:` query
filter to the message list. The toolbar tag action opens a floating tag editor
for the selected message. Adding or removing tags applies immediately through
the existing message keyword mutation path, with optimistic cache updates and
the same event reconciliation used for read and flag changes.

## Sidebar Context Menus

Sidebar objects expose object-scoped right-click menus. Smart mailboxes can be opened or edited in settings. Source account headers can be synced or opened in account settings. Source mailboxes can be opened, can trigger a sync for their parent account, or can open account settings.

## Account Settings

Account settings are edited in a sparse, section-first layout for account identity, appearance, server credentials, verification, sync, and deletion. Mailbox metadata and mailbox actions do not live in the account editor.

The Mailboxes & Rules settings category is a mailbox index for both smart mailboxes and synced source mailboxes. Selecting a mailbox opens a focused mailbox editor page. Smart mailbox editors expose the saved-query definition and backend actions. Source mailbox editors expose server metadata, starting with JMAP role assignment, and backend actions. Role edits are applied immediately through the API, then mailbox, sidebar, and message read-model caches are refreshed through the shared domain cache helper.

Mailbox actions use the shared automation action editor. Each action has its own Save action button; valid actions become active backend automations, while incomplete actions are persisted as drafts and never executed. Smart-mailbox actions are saved as global automations: the selected account condition and smart mailbox rule form the fixed base filter, and each action rule adds its own condition before executing its selected actions. Source mailbox actions are saved as global automations whose fixed base filter is the selected account plus the selected mailbox ID, with each action rule adding its own condition.

## Column configuration

Columns are reorderable (drag-and-drop via dnd-kit), sortable (click header), and resizable (drag right edge via `ColumnResizeHandle`).

`useColumnConfig` manages column visibility, order, sort field/direction, and per-column pixel widths. Sort is sent to the backend message-page query so it applies to the full filtered result set, not only the loaded rows. Available sort fields: `date`, `from`, `subject`, `source`, `flagged`, `attachment`; default is `date` DESC.

Column widths are stored as pixel overrides (`ColumnWidths = Partial<Record<ColumnId, number>>`). Columns without an override use their default CSS grid width from the column definition's `gridWidth`. `buildGridTemplate` accepts optional width overrides and emits pixel values for overridden columns.

All column config (visibility, order, sort, widths) is persisted to localStorage. Header and row cells must share the same effective column widths so resize lines remain visually aligned.

## Live prepend behavior

Incoming domain events are received through `useDaemonEvents`, which dispatches a browser `CustomEvent` used by `MessageList`.

When a relevant event arrives, the current message query is refetched. Scroll offsets are keyed by selected view so refreshes and mailbox switches preserve the user's current position where possible.

## MessageDetail And EmailFrame

`MessageDetail` loads both the selected conversation and the selected message detail. The conversation drives the thread switcher; the message detail drives the currently visible body.

The message switcher intentionally enumerates message summaries inside the selected conversation rather than duplicating the middle-pane list. Messages are deduped by `(sourceId, messageId)` and ordered by `receivedAt`.

When an unread selected message detail successfully loads, the client marks that message as read by adding the JMAP `$seen` keyword through the backend message command API. This is a one-way automatic read transition; explicitly marking a message unread remains a user command.

`EmailFrame` renders wrapped `srcdoc` HTML inside a sandboxed iframe with `allow-same-origin`. It is full-height within the detail body container, so long newsletters scroll inside the iframe rather than forcing the entire right pane to expand. This fixed-height viewport was introduced to solve broken scrolling in long HTML emails. The iframe body background is transparent and the reader body fills the available width, so HTML email does not appear as a fixed-width white document preview unless the email's own HTML explicitly paints one. HTTP and HTTPS links open externally, never inside the app. The iframe has no `allow-scripts`, so a parent-added click handler cannot run (WKWebView blocks it); instead links navigate the top frame (`<base target="_top">` on desktop, `_blank` in the browser). On desktop the Tauri webview's `on_navigation` handler opens any external (`http`/`https`, non-loopback/non-`tauri`) URL in the system browser and blocks the in-app navigation; the app's own loopback/`tauri://` navigations and hash routing pass through. In the browser the link opens a new tab.

When a message has both sanitized HTML and plaintext body alternatives, the
reader renders the HTML alternative. Plaintext is a fallback for messages
without HTML. This matches normal mail-client behavior for
`multipart/alternative` and prevents rendered Markdown email from appearing as
its Markdown source when a provider also supplies the HTML part.

The reader header, attachment strip, and plain text body must follow the L2 visual contract. The metadata header labels incoming messages as addressed to the user, and sent/outgoing messages as addressed to their stored recipients. HTML email may be rendered through an iframe, but the surrounding frame must not dominate the reader or add a default white background.

Attachment rows are preview targets when the MIME type is image, PDF, or text. The preview affordance uses an eye icon, and clicking anywhere on the row outside explicit secondary actions opens a focused attachment surface. Attachment previews are never expanded inline inside the message reader; the focused surface fetches by source, message, and attachment IDs and renders the attachment full-window with a download action.

## Command Search

Command search lives in the global action bar as an icon button that opens the
unified command/search palette. The action bar does not contain an editable
search field. When a query is applied, the current filter is rendered as a
compact chip next to the command-search icon with a clear button.

The palette shows query completions before message and command results when the
current text has a valid completion point. Completion rows update the query text
without closing the panel. Query language help is rendered in the same panel and
uses the same floating-panel behavior as commands and keyboard shortcuts.

Search syntax and backend execution are defined by [L1-search](L1-search.md). The visual treatment is defined by L2.
Message result previews use the global message search endpoint so the palette does not issue one search request per account.

## Settings And Overlays

Settings opens through the shared `SurfaceHost` as a focused settings surface. On web, the host renders the settings panel over the app and exposes an open-in-window control for focused surfaces; on desktop, the same serializable descriptor can be mapped to a native settings window.

The connected accounts list and main sidebar account headers use the account's configured mark as the leading visual identity. Account health is shown separately as a small status dot next to the account name, not as the row's primary icon. When `AccountOverview.syncProgress` is present, the connected accounts list and account editor show the current phase as a compact label with a progress meter. The UI renders backend-provided detail text and mailbox counters, but it does not parse raw logs.

Settings detail pages use shared settings primitives: a centered `SettingsPage`, quiet `SettingsPageHeader`, `SettingsSection` rows with label columns and whitespace, and `SettingsFooter` rows aligned with form content. Nested cards, divider lines, and tabbed subviews are avoided unless a card represents a concrete selectable/list item or nested rule-builder object.

Global appearance settings are persisted through the backend `AppSettings.appearance` object rather than window-local browser state. The theme provider reads `queryKeys.settings`, writes appearance changes through `PATCH /settings`, and relies on `settings.updated` SSE invalidation so separate desktop windows converge on the same theme.

Account editing follows that shared property-page pattern. Identity, server details, and credentials are saved through an Apply footer aligned with the form content. The footer also exposes connection verification and saved/unsaved state. Appearance remains a distinct section on the same page; it uses a single-letter mark with a hue slider and auto-saves for existing accounts. The rendered mark is a solid palette-fitted color, not a translucent badge.

Settings, mailbox editor, shortcuts, onboarding, and compose share the modal principles in L2: centered or top-pinned overlay, restrained glass, fixed dimensions where specified, and no nested card shell unless the card represents a concrete entity. Command search, keyboard shortcuts, and compose use the shared floating panel shell: it sits above the app without a backdrop and can be moved (from the grip or empty header space), resized (from any edge or corner; resize snaps to a static viewport grid, and double-clicking a handle grows to the nearest grid lines — Option/Alt grows the opposite side too), pinned, or expanded so the user can keep reading and interacting with the underlying mail UI. Because the resize handles belong to the shell rather than the panel content, they stay reachable even when the content (such as the command list) would otherwise capture the pointer.

Compose exposes `From` as editable text with suggestions instead of a fixed
select. Suggestions include configured account addresses, the current provider
identity, and backend-cached free-form senders that have previously sent
successfully. The adjacent sender menu shows available suggestions with their
account names; typing a catch-all address is allowed and the provider validates
it during send. Compose loads the accepted free-form cache through
`queryKeys.senderAddresses`; it does not keep sender identities in browser
storage.

Compose body editing is Markdown-first in a single inline-rendered source
editor. Markdown delimiters remain real editable characters while recognized
spans are styled in place. Formatting shortcuts and the editor context menu
insert or remove Markdown markers around the selection, or insert paired markers
at the cursor for the next typed text. Sending still submits the Markdown source
to the Rust API, which emits `text/plain` Markdown plus the rendered `text/html`
alternative.

Focused surfaces are opened from serializable descriptors such as `{ kind: "message", params, disposition: "focused" }`, `{ kind: "attachment", params, disposition: "focused" }`, `{ kind: "settings", params, disposition: "focused" }`, or `{ kind: "compose", params, disposition: "focused" }`. Browser surfaces are represented in the URL hash and rendered as full-window overlays using shared surface content. Browser overlays form a history-backed surface stack: opening an attachment from a focused message pushes a child surface, and Esc or the close button pops only the top surface before returning to the parent message. Desktop surfaces are represented by the same hash routes but opened as native Tauri windows: settings reuses one `settings` window, `o` opens or focuses one stable `message-*` window per exact source/message ID, attachment previews open or focus one stable `attachment-*` window per exact source/message/attachment ID, and compose windows open or focus one stable `compose-*` window per initial compose intent. Surface content fetches by IDs through React Query and must not depend on parent-only React props.

Every desktop window — the main shell and each separate surface window — inherits the same native chrome. On macOS all windows use the inset/overlay title bar (`titleBarStyle: Overlay`, hidden title) so the traffic-light "semaphore" is drawn inside the webview; a shared web abstraction (`WindowChrome`: `TrafficLightInset` for windows that already have a top bar, `WindowTitlebar` for windows that do not) reserves the traffic-light zone and paints the drag region, so the integration is inherited rather than re-implemented per window. The inset values are the single source of truth on the web side and must stay in sync with the Rust `traffic_light_position`. Standard macOS menu shortcuts are provided by native predefined menu items in App/Edit/Window submenus (so they dispatch through the AppKit responder chain and work even while the WebView holds focus); in particular `Cmd+W` closes the focused window via `performClose:`, uniformly for every window. Other desktop platforms keep native window decorations and a custom `Close Window` menu item.

The desktop ships as a single build with webview devtools compiled in (no separate DevTools bundle). Devtools are gated by a client-local "Developer tools" setting (General settings, desktop only, off by default): when on, `Cmd/Ctrl+Alt+I` toggles the focused window's inspector via the `toggle_devtools` command. The installed bundle version is derived from the release tag (`v0.1.0-dogfood.N` → `0.1.N`) rather than a frozen `0.1.0`.

Surface route parameters own visible navigation state inside focused surfaces. Settings category and drill-in state, including account editors, smart-mailbox editors, source-mailbox editors, and create flows, are encoded in the settings surface descriptor. Compose routes encode only the initial compose intent (`new`, `reply`, or `forward`); before the user edits the popup, compose may be opened as a desktop/window surface from that initial intent. Live draft contents remain component/session state and are not teleported between popup and desktop-window shells. Invalid `/surface/...` routes render an explicit closeable invalid-surface state instead of falling back to the mail shell; forced first-run account setup still takes precedence in the main web shell. Component state may hold ephemeral form or pending-operation state, but server/cache refreshes must not replay initial route parameters or otherwise change visible navigation unless they are explicitly correcting an invalid route.

## Keyboard shortcuts

| Key                    | Action                                                                        |
| ---------------------- | ----------------------------------------------------------------------------- |
| `Cmd/Ctrl+K`           | Open command palette                                                          |
| `Cmd/Ctrl+,`           | Open settings                                                                 |
| `Cmd/Ctrl+N`           | Compose new message                                                           |
| `?`                    | Open keyboard shortcuts                                                       |
| `o`                    | Open the selected message in a focused surface                                |
| `Esc`                  | Deselect the open message, or clear the active filter when no message is open |
| `j` / `k` or Down / Up | Next / previous conversation                                                  |
| `e` or `y`             | Archive                                                                       |
| `#` or `Backspace`     | Delete (move to Trash)                                                        |
| `Shift+Cmd/Ctrl+L`     | Toggle flag                                                                   |

The original keyboard plan is broader than the current implementation. The shortcuts above are the ones the frontend actually handles today.

## Keyboard implementation

Keyboard handling is split between shell-level commands and list-level navigation. Window-level handlers must ignore focused inputs and route commands based on the current selection.

The command palette owns its own keyboard state while open. Palette results are
not selected by default after opening or typing. Enter opens the selected row,
while Enter with no selected row applies the current query as a message-list
filter. Shift+Enter and Option/Alt+Enter always apply the query as a filter.
If a typed query has previewed as the active filter, Esc rejects that preview
and clears the filter instead of committing it.
The panel closes on outside interaction unless pinned. List navigation shortcuts
ignore modified key chords, so `Cmd/Ctrl+K` cannot also trigger `k` navigation.

## Undo system

Not implemented yet. Current mutations invalidate and refetch; they do not provide a toast-based undo layer.

## Invariants

- Frontend never talks to JMAP directly; all data flows through the Rust API
- Message list rows come from message endpoints and are not grouped by thread by default
- Persisted email HTML is sanitized in Rust; compose editing renders Markdown source inline without generating frontend-owned email HTML
- Long HTML messages scroll inside the iframe or detail body instead of auto-expanding the pane
- The conversation list preserves scroll position under live prepends
- Keyboard shortcuts do not fire when an input element has focus
- The default UI visual target is the standalone handoff's dark neutral theme
- Coral, blue, and slate-blue remain separate brand/flag, unread, and selection signals
- Main-shell visual details are governed by `docs/L2-ui-visual-reference`
- Focused surfaces are opened through serializable descriptors so web overlays and desktop windows can share the same content components

## Assertions

| ID                               | Sev.   | Assertion                                                                                                                   |
| -------------------------------- | ------ | --------------------------------------------------------------------------------------------------------------------------- |
| ui-no-jmap                       | MUST   | Frontend never makes JMAP calls directly                                                                                    |
| message-list-message-first       | MUST   | Middle pane displays individual messages by default, not grouped thread summaries                                           |
| iframe-sandbox                   | MUST   | Email HTML rendered in sandboxed iframe with no script execution                                                            |
| sanitize-in-rust                 | MUST   | HTML sanitization runs in Rust via ammonia before HTML reaches frontend                                                     |
| tracking-pixel-strip             | SHOULD | 1x1 tracking pixels stripped during sanitization                                                                            |
| anchored-prepend                 | MUST   | Live top-of-list inserts preserve the visible viewport when the user is scrolled down                                       |
| keyboard-input-suppressed        | MUST   | Keyboard shortcuts suppressed when an input, textarea, or editor surface has focus                                          |
| visual-reference                 | MUST   | Main shell and overlay styling conform to `docs/L2-ui-visual-reference` unless a documented backend gap blocks exact parity |
| surface-descriptors-serializable | MUST   | Focused surfaces are described by serializable data, not React component instances or closures                              |
