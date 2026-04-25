---
scope: L1
summary: "React component hierarchy, visual contract boundaries, list behavior, live updates, HTML rendering"
modified: 2026-04-25
reviewed: 2026-04-25
depends:
  - path: docs/L0-ui
  - path: docs/L0-branding
  - path: docs/L0-api
  - path: docs/L1-sync
  - path: docs/L1-search
  - path: docs/L1-compose
dependents:
  - path: docs/L2-ui-visual-reference
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

Mutable account display fields are canonical in the accounts query. Message and sidebar DTOs may contain `sourceName` snapshots, but the UI resolves visible account names from `sourceId -> account.name` through the account directory selector.

Domain events and mutation results update caches through the centralized domain cache helper. Components should not invent ad hoc cache keys or scatter account/message invalidation rules locally. The message list still listens for live domain events and refreshes the current view when a relevant message or mailbox event arrives.

## MessageList

`MessageList` is message-first and currently does manual fixed-row virtualization rather than depending on a virtualization library.

- Row height follows the visual density contract: `24px` compact, `30px` standard, `48px` roomy.
- Rows represent individual `MessageSummary` records, not grouped threads.
- The visible slice is derived from `scrollTop`, `viewportHeight`, and overscan rows.
- Scroll offset is preserved per selected mailbox or smart-mailbox key.
- The active command/search filter, sort field, and sort direction are sent to the message endpoint and executed server-side. The frontend virtualizes loaded pages and fetches the next page near the viewport end.
- Empty list space or `Escape` clears the selected message. When no message is selected, the detail pane is closed so the message list can use the available width.
- The sidebar is resized in a separate shell panel group from the message list and detail pane, so selecting or deselecting a message does not change the left pane width.
- Thread viewing is not the default list mode. When the user wants a thread, a command may apply a thread filter to the message list.

Each row represents one message. The standard density row is tabular, not card-like. It displays unread state, flag state, attachment state, subject, sender, date, account, and tags according to the L2 column contract.

Message rows expose the same primary message actions through a right-click context menu: open, mark read/unread, flag/unflag, archive, and move to Trash. Opening the context menu selects the row first so command targets stay explicit.

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

`EmailFrame` renders wrapped `srcdoc` HTML inside a sandboxed iframe with `allow-same-origin`. It is full-height within the detail body container, so long newsletters scroll inside the iframe rather than forcing the entire right pane to expand. This fixed-height viewport was introduced to solve broken scrolling in long HTML emails.

The reader header, attachment strip, and plain text body must follow the L2 visual contract. HTML email may be rendered through an iframe, but the surrounding frame must not dominate the reader or turn the whole pane into a full-width white document viewer.

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

## Settings And Overlays

Settings opens through the shared `SurfaceHost` as a focused settings surface. On web, the host renders the settings panel over the app; on desktop, the same serializable descriptor can be mapped to a native settings window.

The connected accounts list and main sidebar account headers use the account's configured mark as the leading visual identity. Account health is shown separately as a small status dot next to the account name, not as the row's primary icon.

Settings detail pages use shared settings primitives: a centered `SettingsPage`, quiet `SettingsPageHeader`, `SettingsSection` rows with label columns and whitespace, and `SettingsFooter` rows aligned with form content. Nested cards, divider lines, and tabbed subviews are avoided unless a card represents a concrete selectable/list item or nested rule-builder object.

Account editing follows that shared property-page pattern. Identity, server details, and credentials are saved through an Apply footer aligned with the form content. The footer also exposes connection verification and saved/unsaved state. Appearance remains a distinct section on the same page; it uses a single-letter mark with a hue slider and auto-saves for existing accounts. The rendered mark is a solid palette-fitted color, not a translucent badge.

Settings, mailbox editor, shortcuts, onboarding, and compose share the modal principles in L2: centered or top-pinned overlay, restrained glass, fixed dimensions where specified, and no nested card shell unless the card represents a concrete entity. Command search, keyboard shortcuts, and compose use the shared floating panel shell: it sits above the app without a backdrop and can be moved or pinned so the user can keep reading and interacting with the underlying mail UI.

Focused surfaces are opened from serializable descriptors such as `{ kind: "message", params, disposition: "focused" }` or `{ kind: "settings", params, disposition: "focused" }`. The web host renders focused message surfaces as full-window overlays using the same `MessageDetail` component as the inline reader, and renders settings surfaces with the same `SettingsPanel` content previously used by the dedicated settings overlay. Desktop hosts may map the same descriptors to native windows; surface content must fetch by IDs through React Query and must not depend on parent-only React props.

## Keyboard shortcuts

| Key | Action |
|-----|--------|
| `Cmd/Ctrl+K` | Open command palette |
| `Cmd/Ctrl+,` | Open settings |
| `Cmd/Ctrl+N` | Compose new message |
| `?` | Open keyboard shortcuts |
| `o` | Open the selected message in a focused surface |
| `Esc` | Deselect the open message, or clear the active filter when no message is open |
| `j` / `k` or Down / Up | Next / previous conversation |
| `e` or `y` | Archive |
| `#` or `Backspace` | Delete (move to Trash) |
| `Shift+Cmd/Ctrl+L` | Toggle flag |

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
- Email HTML is sanitized in Rust; frontend renders only sanitized HTML in a sandboxed iframe
- Long HTML messages scroll inside the iframe or detail body instead of auto-expanding the pane
- The conversation list preserves scroll position under live prepends
- Keyboard shortcuts do not fire when an input element has focus
- The default UI visual target is the standalone handoff's dark neutral theme
- Coral, blue, and slate-blue remain separate brand/flag, unread, and selection signals
- Main-shell visual details are governed by `docs/L2-ui-visual-reference`
- Focused surfaces are opened through serializable descriptors so web overlays and desktop windows can share the same content components

## Assertions

| ID | Sev. | Assertion |
|----|------|-----------|
| ui-no-jmap | MUST | Frontend never makes JMAP calls directly |
| message-list-message-first | MUST | Middle pane displays individual messages by default, not grouped thread summaries |
| iframe-sandbox | MUST | Email HTML rendered in sandboxed iframe with no script execution |
| sanitize-in-rust | MUST | HTML sanitization runs in Rust via ammonia before HTML reaches frontend |
| tracking-pixel-strip | SHOULD | 1x1 tracking pixels stripped during sanitization |
| anchored-prepend | MUST | Live top-of-list inserts preserve the visible viewport when the user is scrolled down |
| keyboard-input-suppressed | MUST | Keyboard shortcuts suppressed when an input or textarea has focus |
| visual-reference | MUST | Main shell and overlay styling conform to `docs/L2-ui-visual-reference` unless a documented backend gap blocks exact parity |
| surface-descriptors-serializable | MUST | Focused surfaces are described by serializable data, not React component instances or closures |
