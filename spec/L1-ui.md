---
scope: L1
summary: "React component hierarchy, keyboard model, HTML rendering, search bar"
modified: 2026-03-31
reviewed: 2026-03-31
depends:
  - path: spec/L0-ui
  - path: spec/L0-api
  - path: spec/L1-sync
  - path: spec/L1-search
  - path: spec/L1-compose
dependents: []
---

# UI domain -- L1

## Component hierarchy

```
App
├── QueryClientProvider (React Query)
└── MailLayout (CSS Grid: sidebar | list | detail)
    ├── Sidebar
    │   ├── MailboxList
    │   │   ├── MailboxRow (per mailbox, with unread badge)
    │   │   └── SmartMailboxRow (per smart mailbox, expandable)
    │   └── ComposeButton
    ├── MessageListPanel
    │   ├── SearchBar (always visible)
    │   ├── MessageList (virtualized)
    │   │   ├── MessageRow (flat mode) / ThreadRow (threaded mode)
    │   │   └── EmptyState
    │   └── ListToolbar (sort, thread toggle)
    └── DetailPanel
        ├── ThreadConversationView (multiple messages)
        │   ├── ThreadArcsCanvas (SVG, optional)
        │   ├── MessageCard (per message in thread)
        │   │   ├── MessageHeader (from, to, date, tags — clickable)
        │   │   └── EmailFrame (sandboxed iframe)
        │   └── QuickReplyBar
        ├── SingleMessageView (thread with one message)
        └── EmptyState ("Select a message")
```

## Data fetching

React Query manages all server state. Key queries:

- `useQuery({ queryKey: ['mailboxes'], queryFn: fetchMailboxes })` -- sidebar
- `useQuery({ queryKey: ['emails', mailboxId], queryFn: ... })` -- message list
- `useQuery({ queryKey: ['email', emailId], queryFn: ... })` -- detail view
- `useQuery({ queryKey: ['thread', threadId], queryFn: ... })` -- thread view

Mutations use `useMutation` with optimistic updates. After a mutation, the relevant query is invalidated so React Query refetches automatically.

## MessageList

Virtualized with a library (react-window or similar) to handle mailboxes with thousands of messages. Each row shows: sender, subject, preview (truncated), relative time, unread dot, attachment paperclip, flag star. In threaded mode, rows are grouped by threadId with disclosure triangles. Sort options: date (default), sender, subject, size.

## EmailFrame (sandboxed iframe)

The sanitized HTML (from the Rust API) is rendered in an iframe with `sandbox="allow-popups"` and `srcdoc` attribute. No scripts, no same-origin access. The iframe auto-sizes to content height via a ResizeObserver on the iframe's content document (possible because srcdoc iframes share origin constraints differently from cross-origin ones... actually with sandbox they don't). Alternative: use `postMessage` from a small script injected into the srcdoc to report height. The `sandbox` attribute must include `allow-same-origin` for height measurement to work, but scripts are still blocked by omitting `allow-scripts`. Actually -- the cleanest approach for the vertical slice: set a generous max-height on the iframe and allow scrolling within it. Proper height measurement can be added later.

For now: `<iframe sandbox="" srcdoc={sanitizedHtml} style={{ width: '100%', minHeight: 300 }} />`. Remote images: the API endpoint accepts a query parameter `?loadImages=true` that tells Rust to skip stripping remote image URLs during sanitization.

## Search bar

Always visible at the top of the message list panel. Accepts the full query language from L1-search. Typing shows an autocomplete dropdown (prefix suggestions, mailbox names, contacts, keywords). Enter executes search. Escape clears and returns to mailbox view. Parsed prefixes display as removable chips.

## Keyboard shortcuts

| Key | Action |
|-----|--------|
| `j` / `k` or Down / Up | Next / previous message |
| `Enter` or `o` | Open / expand message |
| `n` | New message (opens compose) |
| `r` | Reply |
| `R` (Shift+r) | Reply all |
| `f` | Forward |
| `e` or `y` | Archive |
| `#` or `Backspace` | Delete (move to Trash) |
| `u` | Toggle read/unread |
| `s` | Toggle flagged/star |
| `l` | Add/edit tags |
| `/` or `Ctrl+F` | Focus search bar |
| `Escape` | Clear search / deselect |
| `g` then `i` | Go to Inbox |
| `g` then `d` | Go to Drafts |
| `g` then `s` | Go to Sent |
| `g` then `a` | Go to All Mail / Archive |

`Cmd+N` and `Cmd+F` conflict with browser shortcuts. Use `n` and `/` instead. Multi-stroke `g` prefix uses a 1-second timeout state machine.

## Keyboard implementation

A `useKeyboardShortcuts` hook at the App level listens for `keydown` events on the document. It checks if the active element is an input/textarea (if so, shortcuts are suppressed). The hook maintains a state machine for multi-stroke sequences. Active panel (sidebar, list, detail) determines which shortcuts are active.

## Undo system

Destructive actions show an undo toast for 5 seconds. The mutation is sent to the API immediately (optimistic). Undo sends a reversal mutation. After timeout, the undo option disappears.

## Invariants

- Frontend never talks to JMAP directly; all data via the Rust API
- Email HTML is sanitized in Rust; frontend renders in sandboxed iframe
- Remote images blocked by default
- Thread grouping uses server-authoritative threadId
- All destructive actions undoable for 5 seconds
- Keyboard shortcuts don't fire when an input element has focus

## Assertions

| ID | Sev. | Assertion |
|----|------|-----------|
| ui-no-jmap | MUST | Frontend never makes JMAP calls directly |
| iframe-sandbox | MUST | Email HTML rendered in sandboxed iframe with no script execution |
| remote-images-opt-in | MUST | Remote images blocked by default; loading requires explicit user action |
| tracking-pixel-strip | SHOULD | 1x1 pixel images stripped from rendered HTML |
| sanitize-in-rust | MUST | HTML sanitization runs in Rust via ammonia before HTML reaches frontend |
| thread-server-auth | MUST | Thread grouping uses JMAP threadId, not client-side heuristics |
| undo-5s | MUST | Destructive actions undoable for 5 seconds via undo toast |
| keyboard-all-actions | SHOULD | Every mouse action also available via keyboard shortcut |
| keyboard-input-suppressed | MUST | Keyboard shortcuts suppressed when an input/textarea has focus |
| dark-mode-inject | MUST | Dark mode CSS injected into every rendered email HTML |
