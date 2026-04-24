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

The query language is parsed in Rust and compiles to the same smart-mailbox rule tree used by saved mailboxes. Free text without a prefix searches sender, subject, and synced preview. Prefixed terms restrict the search to a specific field. Whitespace between tokens is implicit AND. A leading `-` negates the following token.

```peg
Query       <- Token*
Token       <- '-'? (Prefix / FreeText)

Prefix      <- PrefixName ':' Value
PrefixName  <- 'f' / 'from' / 'sender' / 'subject' / 's' / 'body' / 'preview'
             / 'is' / 'has' / 'in' / 'tag'
             / 'keyword' / 'mailbox' / 'source' / 'account'
             / 'before' / 'after' / 'date'
             / 'newer' / 'older'
             / 'id' / 'thread' / 'threadid'

Value       <- QuotedString / DateExpr / RelativeDate / Word
QuotedString <- '"' [^"]* '"'
Word        <- [^\s()]+

DateExpr    <- RelativeDate / AbsoluteDate / NamedDate
RelativeDate <- [0-9]+ [dwmy]             # 2d, 3w, 1m, 1y
AbsoluteDate <- [0-9]{4} '-' [0-9]{2} '-' [0-9]{2}

FreeText    <- QuotedString / Word         # searches sender + subject + preview
```

A query like `from:alice subject:"weekly report" newer:2w` parses into three AND-ed atoms: a from prefix, a quoted subject prefix, and a relative date prefix.

The short `f:` alias is equivalent to `from:`, and `s:` is equivalent to
`subject:`. Text-like prefixes accept optional whitespace after the colon and
consume words until the next recognized prefix, so `f: Posthaste Author` and
`subject: account creation from:posthaste` are valid.

## Prefix mapping

Each prefix compiles to a specific local smart-mailbox field or group of fields.

| Prefix           | Local field                         | Notes                                                              |
| ---------------- | ----------------------------------- | ------------------------------------------------------------------ |
| `f:` / `from:` / `sender:` | `from`                     | Matches address or display name                                    |
| `subject:` / `s:` | `subject`                         |                                                                    |
| `body:` / `preview:` | `preview`                       | Searches synced preview text, not full fetched bodies yet          |
| `is:unread`      | `notKeyword: "$seen"`               | Inverted: unread = absence of `$seen`                              |
| `is:read`        | `hasKeyword: "$seen"`               |                                                                    |
| `is:flagged`     | `hasKeyword: { "$flagged": true }`  |                                                                    |
| `is:unflagged`   | `notKeyword: "$flagged"`            |                                                                    |
| `has:attachment` | `hasAttachment: true`               |                                                                    |
| `in:` / `mailbox:` | `inMailbox`                       | Matches mailbox role, mailbox ID, or mailbox display name          |
| `tag:`           | `hasKeyword`                        | Custom JMAP keywords (Fastmail labels)                             |
| `keyword:`       | `hasKeyword`                        | Alias for `tag:`                                                   |
| `source:` / `account:` | local source condition         | Matches account/source ID exactly or account/source name by text    |
| `before:`        | `before`                            | Exclusive upper bound                                              |
| `after:`         | `after`                             | Inclusive lower bound                                              |
| `date:`          | `after` + `before`                  | Single date = that day (after 00:00, before 23:59:59)              |
| `newer:`         | `after`                             | Relative: `newer:2w` = after (now - 2 weeks)                       |
| `older:`         | `before`                            | Relative: `older:1y` = before (now - 1 year)                       |
| `id:`            | message ID                          | Exact local message ID                                             |
| `threadid:`      | `threadId` filter                   | All emails in a thread                                             |
| `thread:`        | `threadId` filter                   | Alias for `threadid:`                                              |
| Free text        | local text condition                | Searches sender, subject, and synced preview                       |

## Filter compilation

The compiler takes parsed query tokens and produces a `SmartMailboxRule` for server execution against the local SQLite projection. Unsupported prefixes are rejected rather than silently ignored.

Compilation rules:

- Whitespace-separated tokens become an `All` group
- Field aliases such as `f:` and `s:` normalize to the same rule nodes as their long forms
- `-` wraps the token's node or group in negation
- `from:X` desugars to `OR(from_name:X, from_email:X)`
- `is:unread` compiles to `NOT hasKeyword("$seen")` because JMAP tracks "seen", not "unread"
- `in:inbox` desugars to `OR(mailbox_role:inbox, mailbox_id:inbox, mailbox_name contains inbox)`
- `source:X` desugars to `OR(source_id:X, source_name contains X)`
- Date prefixes convert relative expressions to absolute ISO 8601 timestamps at compilation time
- Free text desugars to a local text condition over sender, subject, and synced preview.

The current REST search path compiles query text to the same smart-mailbox rule tree used by saved smart mailboxes, then executes that rule against the local SQLite projection. It intentionally does not maintain a separate frontend search index.

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

### Query help and autosuggest

The command/search palette owns in-app query language help and contextual
autosuggest. Suggestions are derived from local read models, so typing remains
instant and does not introduce a separate backend search path:

- Prefix names: typing `fr` suggests `from:`
- Mailbox names: after `in:`, suggest mailbox names from the local cache
- Contact names: after `from:`/`f:`, suggest addresses from locally cached senders
- Keywords: after `tag:`, suggest known JMAP keywords
- Relative dates: after `newer:` or `older:`, suggest values like `1d`, `1w`, and `1m`
- Exact dates: after `before:`, `after:`, or `date:`, suggest an ISO calendar date

The help surface lists supported prefixes and concise examples inside the same
floating panel. Selecting a suggestion or help row rewrites only the current
query text and keeps the palette open, so users can complete a query without
losing context.

The palette validates query syntax before previewing or applying it. Incomplete
fragments such as `is:` and `from:` are allowed while editing, but they are not
sent to the backend as active filters. Generated value completions must validate
as complete queries before they are shown as selectable query continuations.
The backend parser remains authoritative and validates the same prefixes,
required values, state values, relative dates, and exact dates before executing
message queries.

### Execution pipeline

1. Parse query text into AST
2. Compile parsed tokens to a `SmartMailboxRule`
3. Execute the rule through the backend message-page query path, without collapsing threads by default
4. Display results in the message list with mailbox badges

Search results and mailbox views default to flat mode: individual messages, not collapsed by thread. A thread command may add a `threadId` filter when the user wants to inspect one thread.

### Command palette filtering

`Cmd/Ctrl+K` opens the unified command palette/search panel. The panel floats
above the app without a blocking backdrop and can be moved or pinned, so the
user can keep viewing and interacting with mail underneath it. By default,
clicking outside the panel closes it; pinning keeps it open across outside
interaction. Dragged panel position is persisted locally and restored the next
time the panel opens. While dragging, faint modal-width guide rails appear for
left/center/right and top/bottom placement. When the panel reaches a rail, it
resists movement for a short 12px breakout distance so the user can drag along
the rail; the active rail is highlighted while resisting. As the user types,
matching individual messages are fetched through the backend search endpoint and
shown before commands. The same backend message-page query path also powers the
main message list, so command search and mailbox filtering share query parsing,
filter compilation, sorting, and cursor pagination. While the user types, a
debounced query that the backend accepts may preview as the active message-list
filter; invalid or incomplete query text does not replace the last valid active
filter. No row is selected by default after opening or editing the query. Down
selects the first result, Up from the first result clears selection, Enter opens
the selected result, and Enter with no selected result applies the current query
as a persistent message list filter. Shift+Enter and Option/Alt+Enter always
apply the current query as a filter.

When a message result is selected, the client switches to one of that message's
source mailboxes when the mailbox is known, then opens the message. Applied
filters persist while navigating mailboxes until explicitly cleared. Pressing
Esc with no open message clears the active filter. If the palette has previewed
a typed query as the active message-list filter and the user closes the palette
with Esc before applying it, the preview filter is cleared.

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

| ID                   | Sev.   | Assertion                                                                                  |
| -------------------- | ------ | ------------------------------------------------------------------------------------------ |
| grammar-parse        | MUST   | Every query that conforms to the supported token grammar parses without error              |
| compile-local-rule   | MUST   | Queries using supported prefixes compile to a SmartMailboxRule                             |
| compile-unread       | MUST   | `is:unread` compiles to an unread local rule                                               |
| compile-mailbox      | MUST   | `in:inbox` compiles to a mailbox role or mailbox ID rule                                   |
| compile-date         | MUST   | Relative dates resolve to absolute timestamps at compilation time                          |
| smartmailbox-persist | MUST   | Smart mailbox query_text is persisted and parseable across grammar versions                |
| smartmailbox-refresh | SHOULD | Smart mailbox results refresh on each sync cycle                                           |
| drilldown-click      | MUST   | Clicking a header field populates the search bar with the corresponding prefix query       |
| drilldown-shift      | MUST   | Shift+clicking a header field appends to the current query with AND                        |
| thread-order         | MUST   | Thread conversation view orders messages by receivedAt                                     |
| thread-crossmailbox  | MUST   | Thread view shows all messages in a thread regardless of mailbox                           |
| threadarc-render     | SHOULD | Thread arcs render at 60fps for threads up to 500 messages                                 |
| threadarc-color      | MUST   | Thread arc node colors are consistent per sender within a thread                           |
