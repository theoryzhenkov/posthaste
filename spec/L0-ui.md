---
scope: L0
summary: "Why React, thin-frontend principle, navigation model, rendering approach"
modified: 2026-03-31
reviewed: 2026-03-31
depends:
  - path: README
  - path: spec/L0-api
dependents:
  - path: spec/L1-ui
---

# UI domain -- L0

## Thin frontend principle

The React frontend is a stateless view layer. It fetches data from the Rust API, renders it, and sends mutations back. No business logic, no protocol code, no local database. State management uses React Query for server state (cached API responses with automatic refetching) and local React state for UI-only concerns (selection, focus, panel sizes). The frontend can be tested against a mock API server.

## Why React + TypeScript

React's component model handles complex UI state well -- keyboard shortcut routing, multi-panel layouts, virtualized lists. TypeScript provides type safety for the API contract. The ecosystem offers mature solutions for every UI problem a mail client faces. The alternative (Leptos, Dioxus, or other Rust WASM frameworks) would keep the entire stack in Rust but has a much smaller ecosystem and less mature tooling for complex UIs.

## Why web-first

A browser-based UI eliminates platform-specific code entirely. The same frontend works on macOS, Windows, and Linux. Later, Tauri can wrap it in a native window for desktop distribution. The tradeoff is losing native OS integration (Spotlight, system notifications, mailto: handler), but these can be added via Tauri or a native wrapper without changing the frontend.

## Navigation model

Three-column CSS Grid layout. Left: mailbox sidebar. Center: message list. Right: message detail / thread conversation. This mirrors the traditional mail client layout. Selection flows left-to-right: clicking a mailbox loads its messages, clicking a message shows its content.

## HTML email rendering

Email HTML is sanitized in Rust (via ammonia) before reaching the frontend. The frontend renders it in a sandboxed iframe with `sandbox="allow-popups"` (no scripts, no forms, no same-origin access). Remote images are blocked by default; the user clicks "Load images" to re-request the email with images allowed. Dark mode support is handled by injecting CSS in the Rust sanitization pipeline.

## Keyboard-first design

Same philosophy as the native spec -- every action is keyboard-accessible. j/k navigation, single-key actions, multi-stroke sequences (g+i for inbox). Implemented via a React keyboard event handler at the app root level with a focus management system that routes keys to the active panel.

## What we don't build for v1

Offline mode (Service Worker), push notifications, drag-and-drop, custom themes, print, and mobile-responsive layout. Desktop-first, always-connected.
