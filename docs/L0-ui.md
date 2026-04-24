---
scope: L0
summary: "Why the frontend owns interaction, the handoff-led UI direction, and shell model"
modified: 2026-04-25
reviewed: 2026-04-25
depends:
  - path: README
  - path: docs/L0-api
  - path: docs/L0-branding
dependents:
  - path: docs/L1-ui
  - path: docs/L0-website
---

# UI Domain -- L0

## Thin Frontend Principle

The React frontend is protocol-thin and storage-thin, not interaction-thin. Rust owns JMAP, sync, persistence, sanitization, and authoritative reconciliation. React owns interface behavior: selected view, selected message, per-view scroll offsets, keyboard routing, paginated list state, pane sizes, overlays, command palette state, and anchored live-update behavior.

React Query manages server state. Local React state manages transient UI state.

## Reference Direction

The exported handoff is the visual source of truth. It is not just inspiration. The app should converge toward the standalone reference except where backend functionality is missing.

Reference files:

- `.claude-design/Posthaste.standalone.bundled.html`
- `.claude-design/handoff/posthaste/project/src/tokens.jsx`
- `.claude-design/handoff/posthaste/project/src/prototype.jsx`
- `.claude-design/handoff/posthaste/project/src/window-chrome.jsx`
- `.claude-design/handoff/posthaste/project/src/sidebar.jsx`
- `.claude-design/handoff/posthaste/project/src/message-list.jsx`
- `.claude-design/handoff/posthaste/project/src/reader.jsx`
- `.claude-design/handoff/posthaste/project/src/settings.jsx`
- `.claude-design/handoff/posthaste/project/src/modal.jsx`
- `.claude-design/handoff/posthaste/project/src/command-palette.jsx`
- `.claude-design/handoff/posthaste/project/src/mailbox-editor.jsx`
- `.claude-design/handoff/posthaste/project/src/compose.jsx`

## Why React + TypeScript

React's component model fits a dense mail UI with cross-pane selection, overlays, keyboard routing, column resizing, and long-lived interactive state. TypeScript makes API contracts and UI variants explicit.

The frontend stack should use modern React, TypeScript, Tailwind, shadcn/ui, and focused dependencies where they reduce implementation risk. Familiarity is secondary to matching the reference cleanly.

## Navigation Model

The primary view is a three-pane desktop mail shell:

- Left: sidebar with quick filters, smart mailboxes, tags, and accounts.
- Center: tabular conversation list with resizable columns.
- Right: reader pane with message header, tags, attachment strip, and body.

Selection flows left-to-right. Choosing a mailbox updates the conversation list. Choosing a conversation updates the reader. The reader must not sit blank when conversations are available unless no selection can be derived.

## Layout Modes

The reference supports layout modes, with `layout = 3` as the default. The production target is the three-pane mode first.

Default pane sizes in standard density:

- Sidebar: `210px`
- Message list: `420px`
- Reader: flexes to fill the remainder, with a minimum usable width of `280px`

Compact density changes defaults to:

- Sidebar: `180px`
- Message list: `360px`

Pane splitters are visible `1px` vertical rules with an invisible `8px` hit area. Hover and active states show a `3px` coral line.

## Theme

Dark neutral is the default theme. Light mode remains supported by the token system, but it is not the primary design target for the first parity pass.

The default shell should use layered pane fills and thin separators. It should not use a decorative mesh, radial glow, or large frosted background. Glass blur is reserved for overlays and modals.

## HTML Email Rendering

Email HTML is sanitized in Rust via `ammonia` before reaching the frontend. The frontend renders sanitized HTML in a sandboxed `srcdoc` iframe with `sandbox="allow-same-origin"`. Scripts remain disabled because `allow-scripts` is never granted.

The reader visual contract is set by the handoff. Plain text messages render directly in the reader body with `13px` Geist text and `1.6` line height. HTML messages may use an iframe, but the iframe's outer treatment must preserve the reference reader feel: header first, optional tags, optional attachment strip, then body content on `bgReader` with a `720px` maximum readable body width.

## Keyboard-First Design

The UI is keyboard-first:

- `Cmd/Ctrl+K` opens the command palette.
- `Cmd/Ctrl+,` opens settings.
- `Cmd/Ctrl+N` opens compose.
- `?` opens shortcuts.
- List navigation uses `j/k` and arrow keys.

Shortcut handlers must not fire while an input, textarea, or contenteditable element has focus.

## Live Updates

The UI listens to the daemon EventSource stream and reacts through React Query invalidation plus list-level merge logic.

The conversation list remains paginated and virtualized. Live top-of-list inserts must preserve the visible viewport when the user is scrolled down.

## Backend Gaps

When the handoff includes UI for missing backend capability, the production UI may show a disabled control, placeholder action, or omit the control. The visual structure should still anticipate the handoff target.

Known gaps include schedule send, tracking toggles, AI compose actions, full automation settings, and raw JMAP filter editing.

## What We Do Not Build Into The Shell

The main app view is not a marketing page, dashboard, or onboarding surface. It opens directly into the mail shell.

Mobile-first layout, print styling, plugin UI, full custom theme editing, and offline mutation queue are outside the current UI parity pass.
