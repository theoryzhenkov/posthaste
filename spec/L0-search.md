---
scope: L0
summary: "Why custom query language, search execution strategy, smart mailbox rationale"
modified: 2026-03-29
reviewed: 2026-03-29
depends:
  - path: README
  - path: spec/L0-sync
dependents:
  - path: spec/L1-search
---

# Search domain -- L0

## Why search is the differentiator

MailMate's query language and smart mailboxes are why power users choose it over every other macOS mail client. Boolean search with field prefixes, date arithmetic, and saved queries that behave like virtual mailboxes. No other JMAP client reproduces this. Reimplementing this capability on JMAP is the core value proposition. Without it, this is just another pretty mail client.

## Why a custom query language

JMAP's `Email/query` FilterCondition supports structured filters: from, to, subject, body, date ranges, keywords, mailbox membership, and boolean operators (AND/OR/NOT). This covers roughly 90% of what MailMate offers. But MailMate also supports arbitrary header matching, regex over message bodies, quoted-text-only search, and complex date arithmetic that JMAP cannot express.

A custom query grammar provides a stable user-facing syntax that compiles to JMAP when possible and falls back to local evaluation when not. The grammar is a text string, not a JSON blob, so users can type it directly, paste it between conversations, and save it as a smart mailbox without understanding the underlying protocol.

## Server-first execution

Queries that map entirely to JMAP FilterCondition execute on the server via `Email/query`. The server has indexes; there is no need for the client to maintain a full-text search index for these cases. Queries that include predicates JMAP cannot express (regex, arbitrary header matches) use a split strategy: widen the query for the server to reduce the candidate set, then post-filter locally.

For MVP, all standard prefixes (from, to, subject, body, date, keyword, mailbox) execute server-side. Client-only predicates like `header:` are a v2 feature. This keeps the MVP free of a local full-text index.

## Smart mailboxes

A smart mailbox is a saved query string with display metadata: name, icon, color, sort order, auto-grouping. They are stored locally in GRDB, not synced to the server. JMAP has no concept of client-defined virtual mailboxes, and syncing them would require a custom extension that limits portability.

The query text is the authoritative representation. A parsed AST can be cached for performance, but it is always regenerated from the text when the grammar version changes. This means grammar evolution never invalidates saved smart mailboxes; re-parsing the original text against the new grammar is sufficient.

## Why not expose JMAP filters directly

The user-facing query syntax must be stable across client updates and server changes. JMAP filter JSON is an implementation detail that can change shape as the protocol evolves. A text-based query language also enables: pasting queries between users, keyboard-driven search without a filter builder UI, and grammar evolution without breaking saved smart mailboxes.

Decoupling the query syntax from the protocol also opens the door to supporting non-JMAP backends in the future without changing the user-facing search experience.
