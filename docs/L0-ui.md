---
scope: L0
summary: "Why React, thin-frontend principle, navigation model, rendering approach"
modified: 2026-04-01
reviewed: 2026-04-23
depends:
  - path: README
  - path: docs/L0-api
  - path: docs/L0-branding
dependents:
  - path: docs/L1-ui
---

# UI domain -- L0

## Thin frontend principle

The React frontend is still a thin client, but "thin" here means protocol-thin and storage-thin, not interaction-free. Rust owns JMAP, sync, sanitization, persistence, and authoritative reconciliation. The frontend owns UI interaction state: selected view, selected message, per-view scroll offsets, keyboard routing, paginated list state, and anchored live-update behavior. React Query manages server state; local React state handles transient view concerns.

## Why React + TypeScript

React's component model handles complex UI state well: keyboard routing, multi-panel layouts, paginated lists, and live list updates. TypeScript provides type safety for the API contract. The ecosystem offers mature solutions for the kinds of interaction density a mail client needs.

## Why web-first

A browser-based UI eliminates platform-specific code entirely. The same frontend works on macOS, Windows, and Linux. Later, Tauri can wrap it in a native window for desktop distribution. The tradeoff is losing some native OS integration, but those integrations can be layered on without changing the frontend's data model.

## Navigation model

Three-column CSS Grid layout. Left: mailbox sidebar. Center: conversation list. Right: message detail. Selection flows left-to-right: choosing a mailbox or smart mailbox loads paginated conversations, then choosing a conversation selects its latest message by default.

## HTML email rendering

Email HTML is sanitized in Rust via `ammonia` before reaching the frontend, with tracking pixels stripped as part of the sanitization pass. The frontend renders sanitized HTML in a sandboxed `srcdoc` iframe with `sandbox="allow-same-origin"`. Scripts remain disabled because `allow-scripts` is never granted. The iframe itself forms the scroll viewport for long messages, which avoids auto-height measurement bugs and keeps the detail pane layout stable. HTTPS and `cid:` images that survive sanitization render directly; there is no separate "load remote images" flow yet.

## Keyboard-first design

Same philosophy as the native spec: list navigation and core message actions are keyboard-accessible. The current implementation supports `j/k` and arrow keys for list navigation plus destructive and archive shortcuts from the list/detail context. More of the original keyboard plan can be layered on later without changing the API boundary.

## Live updates

The UI listens to the daemon's EventSource stream and reacts in two ways:

- regular React Query invalidation for sidebar and detail data
- custom list-level merge logic for the paginated conversation pane

This split exists because a paginated, virtualized conversation list cannot be handled well by broad invalidation alone without causing unnecessary refetches or scroll jumps.

## Theme

Light mode first. The color palette, sidebar treatment, and accent colors are defined in [L0-branding](L0-branding.md). Dark mode is deferred — it will be designed later as an inversion of the light theme.

## What we don't build for v1

Offline mutation queue, drag-and-drop, custom themes, print, and mobile-first layout. Desktop-first, always-connected.
