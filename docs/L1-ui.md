---
scope: L1
summary: "React component hierarchy, conversation list behavior, live updates, HTML rendering"
modified: 2026-04-01
reviewed: 2026-04-01
depends:
  - path: docs/L0-ui
  - path: docs/L0-api
  - path: docs/L1-sync
  - path: docs/L1-search
  - path: docs/L1-compose
dependents: []
---

# UI domain -- L1

## Component hierarchy

```
App
├── QueryClientProvider
└── MailClient
    ├── Toolbar
    │   ├── Action buttons
    │   ├── Search input
    │   └── Settings toggle
    └── Main grid
        ├── Sidebar
        │   ├── Smart mailbox section
        │   └── Source mailbox sections
        ├── MessageList
        │   ├── Paginated conversation query
        │   ├── Virtualized visible rows
        │   └── Bottom load-more sentinel
        └── MessageDetail
            ├── Sticky metadata header
            ├── Thread/message switcher
            └── EmailFrame or text fallback
```

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
- Row height is fixed at `78px`.
- Pagination is seek-based using an opaque cursor returned by the backend.
- The visible slice is derived from `scrollTop`, `viewportHeight`, and overscan rows.
- Scroll offset is preserved per selected mailbox or smart-mailbox key.
- Near the bottom of the scroll container, the list fetches the next page.

Each row represents a conversation summary, not an individual message. The row displays the latest sender, subject, preview, relative timestamp, unread state, attachment marker, and message count.

## Live prepend behavior

Incoming domain events are received through `useDaemonEvents`, which dispatches a browser `CustomEvent` used by `MessageList`.

When a relevant event arrives:

- the first conversation page is refetched
- newly arrived top rows are prepended into the cached first page
- if the user is scrolled away from the top, `scrollTop` is increased by `insertedCount * ROW_HEIGHT`

This preserves the visible viewport while still making the new conversation immediately available at the actual top of the list. The user can scroll upward to see it.

## MessageDetail and EmailFrame

`MessageDetail` loads both the selected conversation and the selected message detail. The conversation drives the thread switcher; the message detail drives the currently visible body.

The message switcher intentionally enumerates message summaries inside the selected conversation rather than duplicating the middle-pane list. Messages are deduped by `(sourceId, messageId)` and ordered by `receivedAt`.

`EmailFrame` renders wrapped `srcdoc` HTML inside a sandboxed iframe with `allow-same-origin`. It is full-height within the detail body container, so long newsletters scroll inside the iframe rather than forcing the entire right pane to expand. This fixed-height viewport was introduced to solve broken scrolling in long HTML emails.

## Search bar

Search currently lives in the global toolbar rather than inside the list panel. It is presentational for now and does not yet implement the full structured query experience from L1-search.

## Keyboard shortcuts

| Key | Action |
|-----|--------|
| `j` / `k` or Down / Up | Next / previous conversation |
| `e` or `y` | Archive |
| `#` or `Backspace` | Delete (move to Trash) |
| `s` | Reserved for flag toggle work, not fully wired yet |

The original keyboard plan is broader than the current implementation. The shortcuts above are the ones the frontend actually handles today.

## Keyboard implementation

Keyboard handling lives inside `MessageList` today. A window-level `keydown` listener ignores focused inputs and routes `j/k`, arrow keys, archive, and trash actions based on the current selection. This is intentionally narrower than the earlier global shortcut router design.

## Undo system

Not implemented yet. Current mutations invalidate and refetch; they do not provide a toast-based undo layer.

## Invariants

- Frontend never talks to JMAP directly; all data flows through the Rust API
- Conversation rows come from conversation endpoints, not raw message list endpoints
- Email HTML is sanitized in Rust; frontend renders only sanitized HTML in a sandboxed iframe
- Long HTML messages scroll inside the iframe or detail body instead of auto-expanding the pane
- The conversation list preserves scroll position under live prepends
- Keyboard shortcuts do not fire when an input element has focus

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
