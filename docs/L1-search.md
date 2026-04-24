---
scope: L1
summary: "Query grammar, filter compilation, smart mailbox model, thread arcs, search UX"
modified: 2026-04-24
reviewed: 2026-04-24
depends:
  - path: docs/L0-search
  - path: docs/L1-sync
dependents:
  - path: docs/L1-ui
---

# Search domain -- L1

## Query grammar

The query language uses a PEG grammar parsed in Rust. Free text without a prefix searches across from, to, subject, and body (compiled to JMAP's `text` filter). Prefixed terms restrict the search to a specific field. Boolean logic follows standard precedence: NOT binds tightest, then AND (implicit on whitespace), then OR. Parentheses override precedence.

```peg
Query       <- OrExpr
OrExpr      <- AndExpr ('OR' AndExpr)*
AndExpr     <- NotExpr (NotExpr)*          # implicit AND on whitespace
NotExpr     <- '-' Atom / 'NOT' Atom / Atom
Atom        <- '(' OrExpr ')' / Prefix / FreeText

Prefix      <- PrefixName ':' Value
PrefixName  <- 'from' / 'to' / 'cc' / 'bcc' / 'subject' / 'body'
             / 'participant'               # desugars to OR(from, to, cc)
             / 'is' / 'has' / 'in' / 'tag'
             / 'before' / 'after' / 'date' / 'during'
             / 'newer' / 'older'
             / 'size'
             / 'header'                    # client-only, v2
             / 'id' / 'threadid'

Value       <- QuotedString / DateRange / DateExpr / SizeExpr / Word
QuotedString <- '"' [^"]* '"'
Word        <- [^\s()]+

DateExpr    <- RelativeDate / AbsoluteDate / NamedDate
RelativeDate <- [0-9]+ [dwmy]             # 2d, 3w, 1m, 1y
AbsoluteDate <- [0-9]{4} '-' [0-9]{2} '-' [0-9]{2}
NamedDate   <- 'today' / 'yesterday' / 'thisweek' / 'thismonth' / 'thisyear'
DateRange   <- DateExpr '..' DateExpr

SizeExpr    <- [0-9]+ ('k' / 'K' / 'm' / 'M')   # kilobytes or megabytes

FreeText    <- QuotedString / Word         # searches subject + body + from
```

A query like `from:alice subject:"weekly report" newer:2w` parses into three AND-ed atoms: a from prefix, a quoted subject prefix, and a relative date prefix.

## Prefix-to-JMAP mapping

Each prefix compiles to a specific JMAP FilterCondition property. The mapping is fixed at compile time except for `in:`, which requires resolving mailbox names to IDs against the local cache.

| Prefix | JMAP FilterCondition property | Notes |
|--------|-------------------------------|-------|
| `from:` | `from` | Matches address or display name |
| `to:` | `to` | |
| `cc:` | `cc` | |
| `bcc:` | `bcc` | |
| `subject:` | `subject` | |
| `body:` | `body` | Full-text body search |
| `participant:` | OR(from, to, cc) | Desugars at compilation |
| `is:unread` | `notKeyword: "$seen"` | Inverted: unread = absence of `$seen` |
| `is:flagged` | `hasKeyword: { "$flagged": true }` | |
| `is:draft` | `hasKeyword: { "$draft": true }` | |
| `is:answered` | `hasKeyword: { "$answered": true }` | |
| `has:attachment` | `hasAttachment: true` | |
| `in:` | `inMailbox` | Matches mailbox name or role (inbox, drafts, sent, trash, archive) |
| `-in:` | `inMailboxOtherThan` | Negated mailbox membership |
| `tag:` | `hasKeyword` | Custom JMAP keywords (Fastmail labels) |
| `before:` | `before` | Exclusive upper bound |
| `after:` | `after` | Inclusive lower bound |
| `date:` | `after` + `before` | Single date = that day (after 00:00, before 23:59:59) |
| `during:` | `after` + `before` | Date range: `during:3m..1m` |
| `newer:` | `after` | Relative: `newer:2w` = after (now - 2 weeks) |
| `older:` | `before` | Relative: `older:1y` = before (now - 1 year) |
| `size:` | `size` | `size:>1M`, `size:<100k` with comparison operators |
| `header:` | N/A (client-only, v2) | Arbitrary header match, requires local index |
| `id:` | `Email/get` by ID | Direct lookup, not a query |
| `threadid:` | `threadId` filter | All emails in a thread |
| Free text | `text` (JMAP's combined field) | Searches from + to + subject + body |

## Filter compilation

The compiler takes a parsed query AST and produces a `JmapFilter` for server execution and an optional `LocalPredicate` for client-side post-filtering. For MVP, all supported prefixes compile to JMAP FilterCondition. The `header:` prefix is the only client-only predicate and is deferred to v2.

Compilation rules:

- `AND` nodes become `FilterOperator { operator: "AND", conditions: [...] }`
- `OR` nodes become `FilterOperator { operator: "OR", conditions: [...] }`
- `NOT` / `-` becomes `FilterOperator { operator: "NOT", conditions: [inner] }`
- `participant:X` desugars to `OR(from:X, to:X, cc:X)`
- `is:unread` compiles to `NOT hasKeyword("$seen")` because JMAP tracks "seen", not "unread"
- `in:inbox` resolves the mailbox name or role to a mailbox ID via the local cache before compilation
- Date prefixes convert relative expressions to absolute ISO 8601 timestamps at compilation time
- Free text (no prefix) compiles to JMAP's `text` filter, which searches across all text fields

The mailbox resolution step uses an in-memory map of mailbox names and roles to IDs, maintained by the Rust sync engine (updated on each `Mailbox/changes` cycle). The compiler does not query SQLite at runtime for this; the in-memory map is cheaper and avoids contention. If a mailbox name cannot be resolved, the compiler returns a typed error rather than silently dropping the filter.

## Smart mailbox data model

```
SmartMailbox {
    id: UUID
    account_id: String
    name: String
    query_text: String          # authoritative source
    icon: String?               # SF Symbol name
    color: String?              # hex color
    sort_field: SortField       # receivedAt, from, subject
    sort_order: SortOrder       # ascending, descending
    sub_grouping: SubGrouping?  # optional auto-submailbox
    position: Int               # display order in sidebar
    notify: Bool                # show unread badge
    created_at: Timestamp
    updated_at: Timestamp
}

enum SubGrouping {
    BySender          # group results by from address
    ByYear            # group by receivedAt year
    ByMonth           # group by receivedAt year-month
    ByMailingList     # group by List-Id header (requires local header access)
    ByTag             # group by first keyword/tag
    ByMailbox         # group by mailbox membership
}

enum SortField { receivedAt, from, subject, size }
enum SortOrder { ascending, descending }
```

Persisted in SQLite:

```sql
CREATE TABLE smart_mailbox (
    id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    name TEXT NOT NULL,
    query_text TEXT NOT NULL,
    icon TEXT,
    color TEXT,
    sort_field TEXT NOT NULL DEFAULT 'receivedAt',
    sort_order TEXT NOT NULL DEFAULT 'descending',
    sub_grouping TEXT,
    position INTEGER NOT NULL DEFAULT 0,
    notify INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (account_id, id)
);
```

The `query_text` column stores the raw query string as typed by the user. No compiled or parsed representation is persisted. This allows grammar changes to take effect on existing smart mailboxes without migration.

## Auto-submailbox grouping

When a smart mailbox has a `sub_grouping`, the UI displays it as an expandable node in the sidebar. Expanding it shows dynamically generated child entries based on the grouping field. Each child is the parent query ANDed with an additional filter for the group value.

A smart mailbox "Newsletters" with query `tag:newsletter` and sub_grouping `BySender` shows:

- Newsletters (parent)
  - alice@example.com (query: `tag:newsletter from:alice@example.com`)
  - bob@list.org (query: `tag:newsletter from:bob@list.org`)

Group values are computed by executing the parent query and extracting distinct values of the grouping field from the results. The distinct values are cached in SQLite and refreshed on each sync cycle. This avoids re-executing the parent query every time the sidebar renders.

`ByMailingList` requires access to the `List-Id` header, which is not part of the synced email metadata. This grouping mode depends on locally cached headers from fetched email bodies. It will only produce groups for emails whose bodies have been fetched, which is an acceptable limitation since mailing list emails are typically read and thus already cached.

## Search UX

### Toolbar search

The toolbar search field accepts the full query language. As the user types, the input provides contextual autocompletion:

- Prefix names: typing `fr` suggests `from:`
- Mailbox names: after `in:`, suggest mailbox names from the local cache
- Contact names: after `from:`, `to:`, or `cc:`, suggest addresses from locally cached email participants
- Keywords: after `tag:`, suggest known JMAP keywords
- Named dates: after `before:`, `after:`, or `date:`, suggest `today`, `yesterday`, `thisweek`, `thismonth`, `thisyear`

Completion is local-only, drawing from the SQLite cache. No network requests during typing.

### Execution pipeline

1. Parse query text into AST
2. Compile AST to JMAP FilterCondition (plus optional local predicate for v2)
3. Execute `Email/query` with the filter, without collapsing threads by default
4. Fetch `SearchSnippet/get` for the matching email IDs to get highlighted previews
5. Apply local predicate if present (v2)
6. Display results in the message list with mailbox badges and highlighted snippets

Search results and mailbox views default to flat mode: individual messages, not collapsed by thread. A thread command may add a `threadId` filter when the user wants to inspect one thread.

### Clickable drill-down

Clicking a structured field in the message detail view populates the search bar with the corresponding query. Clicking "alice@example.com" in the From header produces `from:alice@example.com`. Clicking a date produces `date:2026-03-29`. Clicking an attachment icon produces `has:attachment`. Clicking a tag badge produces `tag:tagname`.

Shift+click refines the current query by appending with AND. If the search bar contains `subject:meeting` and the user shift+clicks "alice@example.com" in the From header, the search bar becomes `subject:meeting from:alice@example.com`.

### Search history

The last 50 queries are stored in SQLite with timestamps. Cmd+[ and Cmd+] navigate back and forward through search history. Each history entry stores: query text, result scroll position, and the mailbox context (which mailbox was selected when the search ran). This allows the user to return to a previous search and pick up where they left off.

### Save as smart mailbox

A button in the search results toolbar creates a new SmartMailbox from the current query text. The user provides a name; the rest of the SmartMailbox fields get sensible defaults (default icon, no color, sort by receivedAt descending, no sub_grouping, append to end of sidebar, notifications on).

## Thread view

### Data model

JMAP provides `threadId` on every Email and `Thread/get` returns ordered `emailIds`. The client builds a tree structure from `Message-ID`, `In-Reply-To`, and `References` headers for Thread Arcs only. These headers never change JMAP thread membership. For the flat conversation view (chronological list of messages in a thread), the JMAP-provided order is sufficient.

### Conversation view

The conversation view shows all emails in a thread, ordered by `receivedAt`. Threads are cross-mailbox: a thread spanning Inbox and Sent shows all messages with visual distinction (sent messages right-aligned or tinted differently). On opening a thread, the view auto-expands the selected message, all unread messages, and the newest message. All other messages are collapsed to a single-line summary showing sender and date.

Each expanded message shows sender, date, rendered HTML body, and attachment list. Reply and forward actions are available per-message within the thread.

### Message list threading

Mailbox views default to individual messages, not grouped threads. A later thread command may apply a `threadId` filter to the message list when the user wants to inspect one thread in isolation. The reader may still load the selected message's surrounding conversation for context, but the middle-pane list remains message-first.

Keyboard navigation moves between individual messages.

## Thread Arcs

Thread Arcs (Kerr, 2003) display thread structure as a horizontal baseline with semicircular arcs connecting reply pairs. They provide a compact visual overview of conversation structure, reply depth, participant distribution, and read state.

### Data model

```
ThreadArc {
    nodes: [ThreadArcNode]    # one per email in thread, ordered by date
    arcs: [ThreadArcEdge]     # one per reply relationship
}

ThreadArcNode {
    email_id: String
    sender: String            # for color assignment
    is_read: Bool             # filled vs hollow
    is_in_current_mailbox: Bool  # solid vs dashed outline
}

ThreadArcEdge {
    from_index: usize         # parent node index
    to_index: usize           # reply node index
}
```

### Visual encoding

Nodes are circles on a horizontal baseline. Filled circle means read, hollow means unread. Node color is assigned per sender (consistent color from a palette per sender address within a thread). Dashed outline indicates the message is in a different mailbox than the current view.

Arcs are semicircles drawn above the baseline connecting a parent node to its reply. Arc height is proportional to the distance between connected nodes, which avoids overlapping arcs for adjacent replies while making long-range replies visually distinct. The currently selected message's node is highlighted with a larger size and ring outline.

### Rendering

Rendered as an SVG element (or HTML Canvas) at the top of the thread detail panel. The target is 60fps for threads up to 500 messages. For threads over 500 messages, the view shows a simplified version with arcs only for messages near the current selection. The arc data is computed in Rust and served via the API as a flat array of nodes and edges.

## Assertions

| ID | Sev. | Assertion |
|----|------|-----------|
| grammar-parse | MUST | Every query that conforms to the PEG grammar parses without error |
| grammar-roundtrip | SHOULD | Parsing then serializing a query produces an equivalent (not necessarily identical) string |
| compile-jmap | MUST | Queries using only standard prefixes compile to valid JMAP FilterCondition |
| compile-participant | MUST | `participant:X` compiles to `OR(from:X, to:X, cc:X)` |
| compile-unread | MUST | `is:unread` compiles to `NOT hasKeyword("$seen")` |
| compile-mailbox | MUST | `in:inbox` resolves to the Inbox mailbox ID before compilation |
| compile-date | MUST | Relative dates resolve to absolute timestamps at compilation time |
| smartmailbox-persist | MUST | Smart mailbox query_text is persisted and parseable across grammar versions |
| smartmailbox-refresh | SHOULD | Smart mailbox results refresh on each sync cycle |
| search-snippet | SHOULD | Search results include highlighted snippets from SearchSnippet/get |
| drilldown-click | MUST | Clicking a header field populates the search bar with the corresponding prefix query |
| drilldown-shift | MUST | Shift+clicking a header field appends to the current query with AND |
| thread-order | MUST | Thread conversation view orders messages by receivedAt |
| thread-crossmailbox | MUST | Thread view shows all messages in a thread regardless of mailbox |
| threadarc-render | SHOULD | Thread arcs render at 60fps for threads up to 500 messages |
| threadarc-color | MUST | Thread arc node colors are consistent per sender within a thread |
