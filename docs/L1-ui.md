---
scope: L1
summary: "React component hierarchy, visual contract boundaries, list behavior, live updates, HTML rendering"
modified: 2026-04-23
reviewed: 2026-04-24
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
    ├── ResizablePanelGroup
        ├── Sidebar
        │   ├── Quick filters
        │   ├── Smart mailbox section
        │   ├── Tags section
        │   └── Account mailbox sections
        ├── MessageList
        │   ├── Column header bar (SortableColumnHeader + ColumnResizeHandle)
        │   ├── Paginated conversation query
        │   ├── Virtualized visible rows
        │   └── Bottom load-more sentinel
        └── MessageDetail
            ├── Metadata header
            ├── Tag strip
            ├── Attachment strip
            └── EmailFrame or text fallback
    ├── CommandPalette
    ├── SettingsOverlay
    ├── ShortcutReference
    └── Compose
```

The exact visual contract for these surfaces lives in [L2-ui-visual-reference](L2-ui-visual-reference.md). L1 owns interaction and data rules; L2 owns dimensions, colors, typography, and visual states.

## Data fetching

React Query manages server state, but different surfaces use different strategies:

- `["accounts"]` loads configured account overviews.
- `["sidebar"]` loads enabled sources plus smart mailbox summaries.
- `["conversations", ...viewKey]` uses `useInfiniteQuery` against conversation endpoints and returns `ConversationPage { items, nextCursor }`.
- `["conversation", conversationId]` loads the selected conversation's message summaries.
- `["message", sourceId, messageId]` loads full message detail, including lazily fetched body content when needed.

Mutations still use query invalidation for local actions, but the conversation list does not rely on broad invalidation for live arrivals because it is paginated and virtualized.

## MessageList

`MessageList` is conversation-first and currently does manual fixed-row virtualization rather than depending on a virtualization library.

- Page size is `100`.
- Row height follows the visual density contract: `24px` compact, `30px` standard, `48px` roomy.
- Pagination is seek-based using an opaque cursor returned by the backend.
- The visible slice is derived from `scrollTop`, `viewportHeight`, and overscan rows.
- Scroll offset is preserved per selected mailbox or smart-mailbox key.
- Near the bottom of the scroll container, the list fetches the next page.

Each row represents a conversation summary, not an individual message. The standard density row is tabular, not card-like. It displays unread state, flag state, attachment state, subject, sender, date, account, and tags according to the L2 column contract.

## Column configuration

Columns are reorderable (drag-and-drop via dnd-kit), sortable (click header), and resizable (drag right edge via `ColumnResizeHandle`).

`useColumnConfig` manages column visibility, order, sort field/direction, and per-column pixel widths. Sort is forwarded to the backend via `sort` and `sortDir` query params -- the backend performs the sort, not the frontend. Available sort fields: `date`, `from`, `subject`, `source`, `threadSize`, `flagged`, `attachment`; default is `date` DESC.

Column widths are stored as pixel overrides (`ColumnWidths = Partial<Record<ColumnId, number>>`). Columns without an override use their default CSS grid width from the column definition's `gridWidth`. `buildGridTemplate` accepts optional width overrides and emits pixel values for overridden columns.

All column config (visibility, order, sort, widths) is persisted to localStorage. Header and row cells must share the same effective column widths so resize lines remain visually aligned.

## Live prepend behavior

Incoming domain events are received through `useDaemonEvents`, which dispatches a browser `CustomEvent` used by `MessageList`.

When a relevant event arrives:

- the first conversation page is refetched
- newly arrived top rows are prepended into the cached first page
- if the user is scrolled away from the top, `scrollTop` is increased by `insertedCount * ROW_HEIGHT`

This preserves the visible viewport while still making the new conversation immediately available at the actual top of the list. The user can scroll upward to see it.

## MessageDetail And EmailFrame

`MessageDetail` loads both the selected conversation and the selected message detail. The conversation drives the thread switcher; the message detail drives the currently visible body.

The message switcher intentionally enumerates message summaries inside the selected conversation rather than duplicating the middle-pane list. Messages are deduped by `(sourceId, messageId)` and ordered by `receivedAt`.

`EmailFrame` renders wrapped `srcdoc` HTML inside a sandboxed iframe with `allow-same-origin`. It is full-height within the detail body container, so long newsletters scroll inside the iframe rather than forcing the entire right pane to expand. This fixed-height viewport was introduced to solve broken scrolling in long HTML emails.

The reader header, attachment strip, and plain text body must follow the L2 visual contract. HTML email may be rendered through an iframe, but the surrounding frame must not dominate the reader or turn the whole pane into a full-width white document viewer.

## Search bar

Search lives in the global action bar. In its resting state it behaves like a command-palette entry point: search icon, `Search mail` label, and a mono `Cmd+K` hint. Focused state expands to a structured query input.

Search syntax and backend execution are defined by [L1-search](L1-search.md). The visual treatment is defined by L2.

## Settings And Overlays

Settings opens as a centered sheet over the live mail shell. The main shell remains visible under a dark blur/saturation scrim. Settings must not replace the whole app view.

Command palette, settings, mailbox editor, shortcuts, onboarding, and compose share the modal principles in L2: centered or top-pinned overlay, restrained glass, fixed dimensions where specified, and no nested card shell unless the card represents a concrete entity.

## Keyboard shortcuts

| Key | Action |
|-----|--------|
| `Cmd/Ctrl+K` | Open command palette |
| `Cmd/Ctrl+,` | Open settings |
| `Cmd/Ctrl+N` | Compose new message |
| `?` | Open keyboard shortcuts |
| `j` / `k` or Down / Up | Next / previous conversation |
| `e` or `y` | Archive |
| `#` or `Backspace` | Delete (move to Trash) |
| `Shift+Cmd/Ctrl+L` | Toggle flag |

The original keyboard plan is broader than the current implementation. The shortcuts above are the ones the frontend actually handles today.

## Keyboard implementation

Keyboard handling is split between shell-level commands and list-level navigation. Window-level handlers must ignore focused inputs and route commands based on the current selection.

## Undo system

Not implemented yet. Current mutations invalidate and refetch; they do not provide a toast-based undo layer.

## Invariants

- Frontend never talks to JMAP directly; all data flows through the Rust API
- Conversation rows come from conversation endpoints, not raw message list endpoints
- Email HTML is sanitized in Rust; frontend renders only sanitized HTML in a sandboxed iframe
- Long HTML messages scroll inside the iframe or detail body instead of auto-expanding the pane
- The conversation list preserves scroll position under live prepends
- Keyboard shortcuts do not fire when an input element has focus
- The default UI visual target is the standalone handoff's dark neutral theme
- Coral, blue, and slate-blue remain separate brand/flag, unread, and selection signals
- Main-shell visual details are governed by `docs/L2-ui-visual-reference`

## Assertions

| ID | Sev. | Assertion |
|----|------|-----------|
| ui-no-jmap | MUST | Frontend never makes JMAP calls directly |
| conversation-api | MUST | Middle pane reads from conversation endpoints, not unbounded raw message endpoints |
| iframe-sandbox | MUST | Email HTML rendered in sandboxed iframe with no script execution |
| sanitize-in-rust | MUST | HTML sanitization runs in Rust via ammonia before HTML reaches frontend |
| tracking-pixel-strip | SHOULD | 1x1 tracking pixels stripped during sanitization |
| anchored-prepend | MUST | Live top-of-list inserts preserve the visible viewport when the user is scrolled down |
| keyboard-input-suppressed | MUST | Keyboard shortcuts suppressed when an input or textarea has focus |
| visual-reference | MUST | Main shell and overlay styling conform to `docs/L2-ui-visual-reference` unless a documented backend gap blocks exact parity |
