---
scope: L1
summary: "View hierarchy, thread view, keyboard model, HTML rendering pipeline, search bar"
modified: 2026-03-29
reviewed: 2026-03-29
depends:
  - path: spec/L0-ui
  - path: spec/L0-bridge
  - path: spec/L1-sync
  - path: spec/L1-search
  - path: spec/L1-compose
dependents: []
---

# UI domain -- L1

## View hierarchy

```
MailWindow (NSWindow wrapper)
└── NavigationSplitView
    ├── SidebarView
    │   ├── AccountSection
    │   │   ├── MailboxRow (per mailbox, with unread badge)
    │   │   └── SmartMailboxRow (per smart mailbox, expandable)
    │   └── SmartMailboxSection
    ├── MessageListView
    │   ├── SearchBar (always visible at top)
    │   ├── MessageRow (flat mode) / ThreadRow (threaded mode)
    │   └── Toolbar (compose, archive, delete, toggle thread mode)
    └── DetailView
        ├── ThreadConversationView (multiple messages in thread)
        │   ├── ThreadArcsView (Canvas, optional)
        │   ├── MessageView (per message in thread)
        │   │   ├── MessageHeaderView (from, to, date, tags)
        │   │   └── EmailWebView (WKWebView wrapper)
        │   └── QuickReplyBar (inline reply at bottom)
        └── SingleMessageView (when thread has one message)
```

## MessageListView

Backed by a GRDB `ValueObservation` query that watches the current mailbox or search results. In threaded mode, messages are grouped by `threadId` and each row shows the newest message's subject, participant names, newest date, and unread/total count. In flat mode, individual messages are shown. Default sort is by date; alternative sorts by sender, subject, and size are available.

Swipe actions: archive (left), delete (right). Multi-selection via Shift+Click and Cmd+Click. The list uses lazy loading, so only visible rows are rendered even for mailboxes with tens of thousands of messages.

## ThreadConversationView

Displays all messages in a thread vertically. Each message is a `MessageView` with a collapsible header and an `EmailWebView`. On open, the view applies smart expand/collapse: the selected message, all unread messages, and the newest message are expanded. Everything else is collapsed. Scroll position jumps to the selected message or the first unread message.

## EmailWebView

An `NSViewRepresentable` wrapping WKWebView with the following configuration:

JavaScript is disabled. A custom URL scheme handler intercepts all requests; only `cid:` URLs (inline images from MIME parts) are served, and all other URLs are blocked. Remote images are blocked by default. A "Load remote images" button in the message header re-renders with network access for that specific message only.

After HTML loads, the web view reports its content height to size the SwiftUI frame correctly. This avoids nested scrolling (a scroll view inside a scroll view), which is a common source of broken UX in mail clients that embed web views.

Dark mode support works through an injected CSS stylesheet that sets `color-scheme: light dark` with `@media (prefers-color-scheme: dark)` rules for background, text, and link colors. Emails with explicit background colors are detected and adjusted to prevent white rectangles in dark mode.

## HTML rendering pipeline

This pipeline runs entirely in Rust and is called before passing content to EmailWebView.

1. Parse the raw email body via `mail-parser` to extract the HTML part (or convert text/plain to HTML with paragraph wrapping).

2. Sanitize via `ammonia` with an email-specific allowlist. Allowed tags: `p, br, div, span, a, img, table, tr, td, th, thead, tbody, ul, ol, li, h1-h6, blockquote, pre, code, b, i, u, em, strong, s, sub, sup, hr, center, font`. Allowed attributes: `href, src, alt, width, height, style, class, bgcolor, color, align, valign, colspan, rowspan, border, cellpadding, cellspacing`. The `style` attribute permits only safe CSS properties (color, background-color, font-size, font-family, text-align, margin, padding, border, width, height, display, list-style-type). Properties like position, z-index, and opacity below 0.1 (used for tracking pixels) are stripped. All `a` tags get `target="_blank"` and `rel="noopener noreferrer"`. Image `src` must be `cid:` (inline) or `https://` (remote, blocked unless the user allows). Images that are 1x1 pixels are stripped as tracking pixels.

3. Inject dark mode CSS and a viewport meta tag.

4. Wrap in minimal HTML boilerplate (`<!DOCTYPE html><html><head>...styles...</head><body>...content...</body></html>`).

5. Return as a String to Swift.

## Search bar

The search bar is always visible at the top of MessageListView. It accepts the full query language defined in L1-search. Typing activates an autocomplete dropdown with prefix suggestions, mailbox names, contact names, and keywords. Enter executes the search. Escape clears the query and returns to the previous mailbox view.

Parsed prefixes (e.g., `from:alice`) are displayed as removable chips/tokens in the search bar. The user can always type raw text instead of using the structured tokens.

## Keyboard shortcuts

Default keybindings, following MailMate conventions where applicable:

| Key | Action |
|-----|--------|
| `j` / `k` or Down / Up | Next / previous message |
| `Enter` or `o` | Open / expand message |
| `Cmd+N` | New message |
| `r` | Reply |
| `R` (Shift+r) | Reply all |
| `f` | Forward |
| `e` or `y` | Archive |
| `#` or `Backspace` | Delete (move to Trash) |
| `u` | Toggle read/unread |
| `s` | Toggle flagged/star |
| `l` | Add/edit tags |
| `/` or `Cmd+F` | Focus search bar |
| `Escape` | Clear search / deselect |
| `Cmd+[` | Search history back |
| `Cmd+]` | Search history forward |
| `v` | Move to mailbox (opens picker) |
| `g` then `i` | Go to Inbox |
| `g` then `d` | Go to Drafts |
| `g` then `s` | Go to Sent |
| `g` then `a` | Go to All Mail / Archive |

Multi-stroke sequences use the `g` prefix to enter a "go to" mode. After pressing `g`, the next key selects the destination. This is implemented with a keystroke state machine that tracks the current mode and resets after a 1-second timeout.

## Undo system

Destructive actions (archive, delete, move, tag change) are undoable for 5 seconds. An undo toast appears at the bottom of the window. The action is sent to JMAP immediately (optimistic UI), but the previous state is cached locally. Undo re-applies the previous state via `Email/set`. After 5 seconds, the cached previous state is discarded. Only one undo level is supported: the most recent destructive action.

## Invariants

- UI never calls JMAP directly; all data comes from GRDB, all mutations go through Rust via UniFFI
- HTML rendering pipeline runs entirely in Rust; Swift receives clean HTML
- JavaScript is disabled in all email WebViews
- Remote images are blocked by default
- Thread view groups by JMAP `threadId` (server-authoritative threading)
- All destructive actions are undoable for 5 seconds
- Keyboard shortcuts are configurable via plist (v1 ships with fixed defaults)

## Assertions

| ID | Sev. | Assertion |
|----|------|-----------|
| ui-no-jmap | MUST | UI layer never makes JMAP calls directly |
| webview-no-js | MUST | WKWebView has JavaScript disabled for email rendering |
| webview-no-network | MUST | WKWebView blocks all network requests except explicitly allowed remote images |
| remote-images-opt-in | MUST | Remote images are blocked by default; loading requires explicit user action per message |
| tracking-pixel-strip | SHOULD | 1x1 pixel images are stripped from rendered HTML |
| sanitize-in-rust | MUST | HTML sanitization runs in Rust via ammonia before HTML reaches Swift |
| thread-server-auth | MUST | Thread grouping uses JMAP threadId, not client-side heuristics |
| undo-5s | MUST | Destructive actions are undoable for 5 seconds via undo toast |
| keyboard-all-actions | SHOULD | Every action available via mouse is also available via keyboard shortcut |
| height-no-scroll | SHOULD | EmailWebView measures content height to avoid nested scrolling |
| dark-mode-inject | MUST | Dark mode CSS is injected into every rendered email HTML |
