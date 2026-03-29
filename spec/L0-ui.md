---
scope: L0
summary: "Why SwiftUI, thin-UI principle, navigation model, rendering approach"
modified: 2026-03-29
reviewed: 2026-03-29
depends:
  - path: README
  - path: spec/L0-bridge
dependents:
  - path: spec/L1-ui
---

# UI domain -- L0

## Thin UI principle

The SwiftUI layer is a projection of state held in GRDB. It contains no business logic, no protocol code, no data transformation beyond view formatting. All reads come from GRDB via `ValueObservation`. All writes go through the Rust core via UniFFI. This means the UI can be tested with a pre-populated database and mock Rust objects, with no network dependency.

## Why SwiftUI

SwiftUI is mature enough for a three-column mail client on macOS. It provides `NavigationSplitView` for the standard mail layout, native dark mode, system font rendering, and accessibility out of the box. The alternative, AppKit, offers more control but requires significantly more code for the same result.

WKWebView (AppKit under the hood) handles HTML email rendering, which is the one area where SwiftUI falls short. The rest of the UI is native SwiftUI.

## Navigation model

Three-column split view. Left column: mailbox sidebar with per-account sections and smart mailboxes. Center column: message list, flat or threaded. Right column: message detail or thread conversation view.

The sidebar and message list are SwiftUI native. The message detail uses WKWebView for HTML content. Selection state flows left-to-right: selecting a mailbox populates the message list, selecting a message populates the detail pane.

## HTML email rendering

This is the hardest UI problem. Real-world email HTML is broken, inconsistent, and sometimes hostile. The rendering pipeline works in two stages. First, Rust sanitizes the HTML via `ammonia` (strips scripts, dangerous attributes, scopes CSS) and injects dark mode CSS. Second, Swift renders the clean HTML in a WKWebView with JavaScript disabled and no network access. Remote images are blocked by default, with a per-message "Load images" button.

Sanitization runs in Rust so that Swift never touches raw, potentially malicious HTML. The WKWebView is locked down as a pure rendering surface.

## Keyboard-first design

Following MailMate's philosophy, every action is keyboard-accessible. Single-key shortcuts handle common actions (archive, delete, reply, forward). Multi-stroke sequences handle less common actions. The initial shortcut set follows MailMate conventions where possible, and shortcuts are configurable via plist files.

## What we don't build for v1

Custom themes, toolbar customization, notification preferences, multiple window support, full-screen compose, print, share extensions, Spotlight indexing, and `mailto:` handler registration. These are real features but not required for the MVP.
