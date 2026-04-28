---
scope: L0
summary: "Why Markdown composition, MIME strategy, borrowed components"
modified: 2026-04-28
reviewed: 2026-04-28
depends:
  - path: README
  - path: docs/L0-jmap
  - path: docs/L0-sync
dependents:
  - path: docs/L1-compose
---

# Compose domain -- L0

## Why Markdown

MailMate proved that power users prefer Markdown composition over traditional rich text editors. Markdown is plain text: versionable, greppable, portable. It converts to well-structured HTML with predictable output. The alternative, authoring mutable HTML directly, is far more complex and produces worse HTML.

The compose UI is an inline-rendered Markdown source editor. Markdown delimiters remain real document characters, but the editor styles recognized spans in place: `*text*` looks italic, `**text**` looks bold, code spans use monospace styling, and strikethrough is shown inline. Deleting still deletes the real Markdown characters first, so removing a closing `*` turns the span back into literal Markdown source instead of mutating hidden HTML state.

## MIME strategy

Outgoing email uses `multipart/alternative` with `text/plain` (the Markdown source) and `text/html` (the rendered output). Recipients with plain-text clients see readable Markdown. Recipients with HTML clients see formatted content. Attachments wrap the alternative part in `multipart/mixed`. This is standard RFC 2046 structure, understood by every mail client.

Preserving the Markdown source as the plain text part is a deliberate choice. Most rich text editors generate garbage `text/plain` parts by stripping HTML tags. Here the plain text part is the original authored content and reads naturally on its own.

## Borrowed: pulldown-cmark

`pulldown-cmark` is a Rust crate implementing CommonMark with GFM extensions (tables, strikethrough, task lists). It parses email-length text in sub-millisecond time and is widely used across the Rust ecosystem. The send pipeline calls it from Rust to generate the final HTML alternative. The browser editor may use a separate Markdown parser for inline styling, but the Rust renderer is authoritative for outgoing MIME.

## Draft lifecycle via JMAP

Drafts are real JMAP Email objects with the `$draft` keyword, stored on the server via `Email/set`. This means drafts sync across devices automatically with no additional infrastructure. The compose session operates on local state and periodically saves to the server. Sending uses `EmailSubmission/set`, whose `onSuccessUpdateEmail` argument can request the server-side update that clears or moves the draft after successful submission.

## What we don't build

No HTML source editing and no hidden rich-text document model. Formatting commands insert or remove Markdown markers around the selected source text; they do not create HTML nodes. This is a deliberate constraint that keeps composition portable and the output clean. Users who need pixel-perfect HTML email formatting are not the target audience.
