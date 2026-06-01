---
scope: L1
summary: "Plan for a provider-owned, bounded, Raycast-like command palette/search architecture"
modified: 2026-06-01
reviewed: 2026-06-01
lifecycle: ephemeral
type: PLAN
tags:
  - gpt-5.5-high
  - claude-opus-4-8-redteam
depends:
  - path: README
  - path: docs/L1-ui
  - path: docs/L1-search
  - path: docs/L1-api
  - path: docs/L1-logging
---

# Command Palette Search Architecture Plan

Posthaste should use a bounded retrieval and reranking pipeline, not a client-side global scan. The architecture is the standard information-retrieval shape for this problem: providers retrieve a small, relevant candidate page from the right source; a coordinator manages a search session; a deterministic ranker blends only loaded candidates; the UI renders a hybrid of top hits and explainable sections. Context personalizes those bounded candidates. It never decides what messages exist.

The design below intentionally changes parts of the suggested plan to fit the current codebase. Posthaste already has a typed `/read` bootstrap, cursor-paginated message endpoints, React Query message-list pagination, and a global `/v1/messages/search` endpoint. The first refactor should reuse those boundaries.

Relevance-ranked retrieval is a backend concern, and the long-term goal is a *single* search backend, not two. A `/messages/search-preview` sibling may be introduced as a temporary staging endpoint while FTS is built, but the end state is one relevance-capable `/messages/search` that takes a `sort=relevance|date` parameter and serves both the message list and the palette. Two endpoints with different retrieval engines would make the same query language return different results in the list versus the palette, and double the authorization and maintenance surface. See §2 *Backend message search* and the backend-search phase in §6.

## 1. Current code findings

### Relevant frontend files

- `apps/web/src/components/CommandPalette.tsx` owns the floating panel, query input, keyboard selection, provisional message-list preview, and a React Query call to `fetchSearchMessages(..., { limit: 8 })`.
- `apps/web/src/hooks/useCommandPaletteResults.tsx` builds grouped rows in one hook. It mixes commands, query completions, message results, contacts derived from message results, and mailbox results. It also hard-caps messages, contacts, and mailboxes with `.slice(...)`.
- `apps/web/src/queryLanguage.ts` and `apps/web/src/queryDefinitions.ts` implement local query validation, prefix help, and completions from local read models plus currently loaded message preview rows.
- `apps/web/src/mailboxNavigationReadModels.ts` runs the typed `POST /read` bootstrap and hydrates normalized React Query caches for accounts, mailboxes, smart mailboxes, and tags.
- `apps/web/src/components/MessageList.tsx` already has the right pagination shape for message lists: `useInfiniteQuery`, backend cursor pages, cancellable fetches through React Query `signal`, and manual fixed-row virtualization.
- `apps/web/src/api/client.ts` has the existing REST client functions: `read`, `fetchSourceMessages`, `fetchSmartMailboxMessages`, and `fetchSearchMessages`.
- `apps/web/src/queryKeys.ts` names the existing React Query cache roots. New command-search keys should live here or in a small adjacent command-search module.
- `apps/web/src/observability.ts`, `apps/web/src/logger.ts`, and `apps/web/src/logEvents.ts` provide operation/request correlation and typed frontend logging.
- `apps/web/src/surfaces.ts` and `apps/web/src/hooks/useSurfaceRouting.ts` define focused surface navigation for messages, settings, attachments, and compose.

### Relevant backend files

- `crates/posthaste-server/src/lib.rs` registers `/v1/messages/search` next to source and smart-mailbox message routes. A future `/v1/messages/search-preview` belongs in the same route group.
- `crates/posthaste-server/src/api.rs` implements `search_messages`, `read`, and shared query parsing helpers such as `parse_optional_search_rule`.
- `crates/posthaste-server/src/authz.rs` lists route authorization templates. Any new `/messages/search-preview` route must be added there.
- `crates/posthaste-store/src/read.rs` and `crates/posthaste-store/src/smart_mailboxes.rs` provide the current seek-paginated `MessagePage` queries.
- `crates/posthaste-store/src/db.rs` shows the local SQLite projection. Messages, mailboxes, keywords, attachments, sender address cache, and cache signals already exist. There is no dedicated contacts table yet.
- `crates/posthaste-domain/src/search.rs` contains the Rust query parser/compiler used by backend message search and smart mailbox rules.

### Current grouping, caps, and “show more” state

No explicit `Show more` command-palette layer was found. The current palette is grouped only: Suggestions, Messages, Query Language, Commands, Contacts, Mailboxes. It hard-caps loaded groups in the UI builder (`messages.slice(0, 8)`, contacts `slice(0, 5)`, mailboxes `slice(0, 6)`). That is acceptable as a temporary UI constraint, but it should not become the architecture.

The pieces to remove or replace in the refactor are:

- `useCommandPaletteResults` as the central search implementation. Keep only small row-rendering helpers if useful.
- Contacts derived from the current message preview page. That is not a contacts provider; it hides the absence of a real contact/correspondent read model.
- UI-owned group caps as the only way to control search scale. Provider `limit` values should own retrieval bounds; row rendering may still choose how many to display.
- Pure grouped presentation. Replace it with a hybrid result model: Best matches plus vertical sections.

The existing `fetchSearchMessages` call is not the wrong direction. It is already bounded, cancellable via `AbortSignal`, and backend-owned. It should move behind a `MessageSearchProvider` and a shared message-page client abstraction.

### Startup and shell behavior

The desired invariant is that the shell and command palette are available before `/read` finishes. The current `MailClient` still blocks on `accountsQuery.isLoading || mailNavigationBootstrap.isLoading` and renders a full “Setting up...” state before the action bar and palette can appear. That should be fixed before or during the command-search refactor. The command palette must not depend on mail-navigation read models to open. It can show commands immediately, then add mailboxes/tags/messages as providers become ready.

Preserve the parts that are already safe:

- `CommandPalette` is lazy-loaded, which protects initial bundle work. If this causes a blank first-open pause, prefetch the chunk after idle rather than bundling it into critical startup.
- `/read` is a typed bootstrap that hydrates domain caches. Keep it; do not replace it with UI-shaped bespoke endpoints.
- Message list data loading is already backend-paginated and virtualized.

### Existing local stores and caches

- Accounts: backend config/read model, `queryKeys.accounts`, `queryKeys.account(id)`.
- Mailboxes: SQLite `mailbox`, `/sources/{source_id}/mailboxes`, `Mailbox/list` in `/read`, `queryKeys.mailboxes(accountId)`.
- Smart mailboxes: config-backed smart mailbox APIs, `SmartMailbox/list` in `/read`, `queryKeys.smartMailboxes`.
- Tags: SQLite-derived `message_keyword` aggregation, `Tag/list` in `/read`, `queryKeys.tags`.
- Messages: SQLite `message`, `message_mailbox`, `message_keyword`, `message_attachment`, `message_body`; backend cursor pages; React Query infinite pages in `MessageList`.
- Contacts: no first-class contact store. Compose suggestions derive account addresses and recent correspondents from conversation pages; `sender_address_cache` stores accepted free-form sender addresses, which is sender identity cache, not a general contacts index.

### Existing backend API shape

Posthaste prefixes JSON endpoints with `/v1`. Message pages use `MessagePageResponse { items, nextCursor }` and opaque seek cursors. The current global message search is:

```http
GET /v1/messages/search?q=...&cursor=...&limit=...&sort=...&sortDir=...
```

It parses query language to a `SmartMailboxRule`, then executes SQLite queries over the local projection. Text search currently compiles to `LOWER(...) LIKE '%term%'` over sender, subject, source, mailbox names, and preview fields. That is fine for MVP list filtering, but it is not the long-term command-palette retrieval engine for tens of thousands of messages.

A future preview endpoint fits here:

```http
GET /v1/messages/search-preview?q=...&cursor=...&limit=...
```

It should be added to `api.rs`, registered in `lib.rs`, documented in `docs/L1-api.md`, included in OpenAPI/type generation, and authorized in `authz.rs`. The first implementation can adapt `/messages/search`; the FTS implementation should eventually own relevance ordering and return match evidence.

### Existing navigation APIs

- Open a source mailbox: set `selectedView` to `{ kind: 'source-mailbox', sourceId, mailboxId, name }`.
- Open a smart mailbox: set `selectedView` to `{ kind: 'smart-mailbox', id, name }`.
- Open a message in the main shell: set `selectedMessage` from `MessageSummary` IDs. The current palette also switches to one source mailbox when it can resolve one from `message.mailboxIds`.
- Open a focused message surface: `openFocusedSurface(messageSurfaceFromSelection(selection))`.
- Open settings: `openFocusedSurface(settingsSurface(...))` and helpers for account/smart/source-mailbox targets.
- Open compose: `ComposeIntent` through `useComposeIntent` or `composeSurface(intent)`.
- Contacts have no route or surface yet. Until a contact read model exists, contact rows should apply a query such as `from:<address>` or open a future contact surface only when that surface exists.

## 2. Clear architecture

### Providers

Search providers are independent candidate retrievers. Each provider knows its own source, limits, cursor semantics, and match evidence. Providers return candidates, not final UI rows.

Initial providers:

- `CommandProvider`: local, small, synchronous/in-memory. Build it against the current static command list (the one in `useCommandPaletteResults`). The contextual-action registry is planned but not yet built, so do not depend on it; migrate `CommandProvider` onto the registry once it exists rather than sequencing this work behind it.
- `QueryCompletionProvider`: local, small. Wraps existing `queryLanguage.ts` completion/help logic.
- `MailboxProvider`: local read-model provider over accounts, source mailboxes, and smart mailboxes from `/read`-hydrated caches.
- `TagProvider`: local read-model provider over `queryKeys.tags`.
- `ContactProvider`: initially a thin bounded provider over known local sources only: recent correspondents if a read model exists, configured account addresses where useful, and later a backend `Contact/list` or correspondent cache. Do not synthesize a contact universe from the first page of message results.
- `MessageSearchProvider`: backend-paginated provider. Initially adapts `fetchSearchMessages`; later switches to `/messages/search-preview`.

Providers may be temporarily unavailable. The coordinator should render local command/query providers immediately and mark mailbox/tag/contact/message providers as loading, empty, or unavailable without blocking the palette.

### Shared message page abstraction

MessageList and MessageSearchProvider should share message-page retrieval mechanics without sharing UI state. Create a lower-level client abstraction around the existing endpoints:

```ts
type MessagePageScope =
  | { kind: 'source-mailbox'; sourceId: string; mailboxId: string | null }
  | { kind: 'smart-mailbox'; smartMailboxId: string }
  | { kind: 'global' }
  | { kind: 'preview' };

interface MessagePageRequest {
  scope: MessagePageScope;
  query?: string;
  cursor?: string | null;
  limit: number;
  sort?: MessageSortField | 'relevance';
  sortDir?: 'asc' | 'desc';
  signal?: AbortSignal;
  operation: OperationContext;
}

interface MessagePageClient {
  fetchPage(req: MessagePageRequest): Promise<MessagePage>;
}
```

`MessageList` keeps `useInfiniteQuery`, scroll restoration, row virtualization, and user-selected sort. `MessageSearchProvider` uses the same `MessagePageClient` with `scope: { kind: 'global' }` now and `scope: { kind: 'preview' }` later. This is the right DRY boundary: shared API pagination and cursor mechanics, separate presentation/session behavior.

Do not force command palette search into the message-list `useInfiniteQuery` hook. The palette is a multi-provider search session, not a list view.

### Coordinator

Introduce `SearchCoordinator` as a pure session controller, exposed to React as `useCommandSearch()`. It should:

- receive the query and open reason;
- capture one immutable `RankingContext` snapshot per query version;
- choose providers based on query and provider readiness;
- issue bounded provider requests with per-provider limits;
- own an `AbortController` for the current query version;
- abort stale requests on query change and palette close;
- ignore late responses whose query version no longer matches;
- maintain provider states, candidate reservoirs, cursors, latencies, and errors;
- blend loaded candidates into rows;
- expose `loadMore(providerId)` and optionally `loadMoreVisibleProviders()` for scroll pagination.

This should be implemented as framework-light TypeScript logic with a React hook wrapper. Core ranker/coordinator tests should not need DOM rendering.

### Result stability and settlement

Async providers create a tension: the fastest provider (local mailboxes/commands) returns first and the slowest (backend message search — often the thing the user is actually after) returns ~100-250 ms later. If Best Matches freezes on first paint, the best result can be locked out of the top. If it never freezes, rows churn under the user's cursor.

Resolve it with an explicit settlement contract, not a vague "after it settles":

- A query version is *settled* when every provider selected for that query has either returned a first page or exceeded a settlement deadline (target 250 ms). Local providers settle synchronously.
- Best Matches may reorder freely until the query version is settled.
- After settlement, Best Matches order is frozen for that query version. New candidates from later pages append to their vertical sections; they do not reorder Best Matches.
- Selection is always pinned by candidate ID, and freezing is anchored to the user's current selection, not row 0. A late, higher-ranked candidate never displaces the row the user has selected or moves rows above it.
- A new keystroke starts a new query version and a fresh settlement window.

### Ranking context

`RankingContext` is a snapshot of local state. It is bounded, redacted where possible, and used only after providers have returned candidates.

Context examples:

- Current route and selected message/thread/mailbox.
- Palette open reason.
- Recent/frequent commands and entities, stored as decayed counters.
- Pinned/favorite commands or mailboxes.
- Provider readiness and index versions.

Context must not contain message bodies. It must not ask the client to enumerate the message corpus. It can pass small safe hints to a backend preview search, such as current account/mailbox or selected thread/contact IDs, but backend retrieval remains indexed and paginated.

### Ranker and blender

Build a deterministic ranker first. Provider raw scores are not comparable — BM25 from messages, fuzzy scores from commands, and decayed history scores mean different things — so the ranker never compares them directly.

For v1, rank with a **lexicographic tier system**, not a weighted sum. A weighted sum needs calibrated weights, and there is no labeled data or impression log to calibrate against at launch, so the weights would be guesses and tuning them would be a rabbit hole. Tiers have obvious cold-start defaults and are debuggable by inspection:

1. **Tier 1 — strong explicit match.** Exact or prefix match on a candidate's primary label, in any vertical. Within the tier, order by match strength (exact before prefix), then by context.
2. **Tier 2 — contextual and vertical relevance.** No strong label match. Order by vertical prior, then context boosts (current mailbox/thread, recency/frequency, pinned/favorite), then within-provider rank.
3. **Tier 3 — weak match.** Fuzzy/contains/acronym only. Order by match quality, then context.

Context's influence scales with query strength: heavy on an empty query, moderate at one or two characters, a tie-breaker on a strong exact/prefix query, and more helpful on a fuzzy/ambiguous query. In the tier model this is expressed by *which tier* a candidate lands in (a strong query pushes exact matches into Tier 1, where context is only a tie-breaker) rather than by reweighting a sum.

Capture the same typed features regardless (match kind, vertical, context signals, within-provider rank) so they are logged from day one. A weighted blend — and later a model that reranks only the top loaded candidates — can replace the tier comparator once impression and selection logs exist to evaluate it. Do not build an ML model now.

### UI rows and virtualization

The coordinator returns a flattened row list:

```ts
type PaletteRow =
  | { kind: 'section'; id: string; label: string }
  | { kind: 'item'; id: string; candidate: SearchCandidate }
  | { kind: 'loading'; id: string; providerId: string }
  | { kind: 'empty'; id: string; providerId: string }
  | { kind: 'error'; id: string; providerId: string; message: string };
```

Only `item` rows are selectable. `section`, `loading`, `empty`, and `error` rows are skipped by arrow navigation and ignored by Enter. Keyboard navigation operates over the currently rendered row snapshot at keypress time: selection is held by candidate ID, "first" means the first `item` row at that moment, and the existing semantics are preserved — no default selection after open or edit, Down selects the first item, Up from the first item clears, and Enter with no selection applies a valid query.

Do **not** virtualize in v1. Providers are bounded, so the palette renders on the order of tens of rows, not thousands; the current `cmdk`-based rendering handles that fine. Keep `cmdk` for the input, list, selection, and its accessibility wiring (roles, `aria-activedescendant`), and render coordinator-computed rows through it instead of the old static groups. Manual virtualization plus a hand-rolled active-descendant model is a meaningful rewrite with real a11y cost and no proven benefit here. Revisit virtualization only if profiling shows a concrete problem; it is rendering hygiene, not search architecture.

### Backend message search

Long-term message retrieval should use SQLite FTS5 first, because the project already owns a SQLite local replica through `rusqlite`. Tantivy can be reconsidered later if FTS5 is not enough, but adding a second indexing engine now would increase operational cost.

Treat FTS as the *single* search backend, not a second engine alongside the existing `LIKE` query. The current `search_messages` compiles the query language to `LOWER(COALESCE(col,'')) LIKE '%term%'` over from-name/from-email/subject/preview, sorted by date with no relevance (`api.rs` → `smart_mailboxes.rs`). The end state is one relevance-capable `/messages/search` that accepts `sort=relevance|date`; the message list keeps `sort=date` and the palette requests `sort=relevance`. Any `/messages/search-preview` sibling is a temporary staging step that must be retired onto the unified endpoint, so the list and palette never run two different retrieval engines over the same query language.

The backend should maintain a search index keyed by `(account_id, message_id)` with indexed fields such as sender/from, subject, recipients, attachment filenames, preview/body text, tags, and mailbox/source names. Field weights should be roughly:

- sender/from: very high;
- subject: high;
- recipients: medium-high;
- attachment filenames: medium;
- preview/body: normal;
- quoted body: low or excluded.

The endpoint should return a preview-shaped page with opaque cursor, candidate summaries, and match evidence. It should not use `%LIKE%` scans as its steady-state implementation.

FTS5 is a semantics migration, not an endpoint adapter. Three issues must be designed up front, because they change what the query language matches:

- **Tokenization of email addresses.** The default `unicode61` tokenizer splits on `@` and `.`, so `from:alice@example.com` will not behave like today's substring match. Use a dedicated, exact-matched address column (or a custom tokenizer) for `from:`/`to:` operators rather than relying on full-text tokens.
- **Prefix queries.** Type-ahead needs prefix matching (`repor*`), which requires `prefix=` configured at index creation.
- **Index sync.** An external-content FTS table must be kept in sync with `message` via insert/update/delete triggers (or explicit maintenance in the projection layer). A desynced index silently returns wrong results.

The query-language operators currently mean substring `LIKE`; FTS token-match has different recall. Decide per operator whether it maps to a token match or an exact-column match, and update `crates/posthaste-domain/src/search.rs` and the `L1-search` spec accordingly.

Authorization is a data-scoping requirement, not just a route entry. Index queries must filter to the caller's authorized `account_id` set; preview results must never cross account boundaries the caller is not authorized for.

## 3. Explicit decisions

### Grouped vs global

Use a hybrid presentation.

Under the hood, compute a global blended rank over loaded candidates. In the UI, render:

1. Best matches: the global top 5-10 loaded candidates.
2. Messages.
3. Contacts.
4. Mailboxes.
5. Tags.
6. Commands.
7. Query language, when relevant.

Deduplicate Best Matches from lower sections by default. Repeating the same row makes keyboard navigation and screen-reader output noisier. If later usability testing shows users rely on seeing the same message under its vertical, add a small “also shown in Best matches” placeholder instead of duplicating the actionable item.

### Empty query shows recents

On an empty query the palette shows commands, query help, and a small set of *recents*: recently selected commands/entities from local history, and recent messages/threads from data already in the React Query cache (e.g. the current mailbox page that `MessageList` loaded). Reading already-loaded cache is not a global scan and does not violate the no-client-scan rule — the rule forbids *loading or ranking the whole corpus*, not reusing context already in memory. The message provider is not called against the backend on an empty query.

### Minimum query length

Free-text backend search has a hard minimum (2-3 characters) before the message provider issues a backend request; a single character maps to `LIKE '%a%'` (or a near-empty FTS term) over the whole corpus and is useless. Incomplete query-language operator prefixes such as `from:` with no value stay local and drive completions only. Local providers (commands, query help, mailboxes, tags) may match from the first character.

### Pagination

Pagination is provider-owned. The coordinator may rebalance loaded candidates, but it never paginates the global universe by revealing hidden prebuilt rows. Scroll pagination calls providers for their next pages, appends those pages to provider reservoirs, and reranks the loaded reservoir only.

Scroll stability follows the settlement contract in §2 *Result stability and settlement*: selection is pinned by candidate ID, Best Matches freezes once the query version is settled (all selected providers returned or the 250 ms deadline elapsed), and later pages append inside their vertical sections rather than reordering Best Matches or moving rows above the user's selection.

### Context

Context reranks bounded candidates only. It does not retrieve messages. It does not scan local mail. It does not override a strong explicit query.

### ML

No ML or predictor model in the first implementation. Add deterministic ranking, typed features, impression logging, selection logging, and replay first.

### Messages

Message retrieval belongs on the backend. The first provider reuses `/v1/messages/search` as-is; the end state is the unified relevance-capable `/messages/search` (`sort=relevance`) backed by FTS, with any `/messages/search-preview` used only as a temporary staging endpoint (see §2 *Backend message search*). The client must never load all messages across mailboxes to rank them.

### DRY with MessageList

Share API-level message page retrieval, cursor handling, request correlation, query normalization, and generated types. Do not share UI hooks, row components, scroll state, or ranking logic between MessageList and command search.

## 4. Type/interface proposal

```ts
export type SearchVertical =
  | 'command'
  | 'query-completion'
  | 'mailbox'
  | 'tag'
  | 'contact'
  | 'message';

export interface SearchProvider {
  id: string;
  label: string;
  vertical: SearchVertical;
  search(req: ProviderSearchRequest): Promise<ProviderResultPage>;
}

export interface ProviderSearchRequest {
  query: string;
  cursor?: string | null;
  limit: number;
  context: RankingContext;
  signal?: AbortSignal;
}

export interface ProviderResultPage {
  candidates: SearchCandidate[];
  nextCursor: string | null;
  indexVersion?: string;
  latencyMs?: number;
}

export interface SearchCandidate {
  id: string;
  providerId: string;
  vertical: SearchVertical;
  entry: CommandPaletteEntry;
  providerRank: number;
  providerScore?: number;
  match: MatchEvidence;
  features: SearchFeatureMap;
}

export type SearchFeatureMap = Record<string, number | boolean | string>;

export interface MatchEvidence {
  query: string;
  fields: Array<{
    field: 'label' | 'subtitle' | 'keywords' | 'from' | 'subject' | 'body' | 'mailbox' | 'tag';
    kind: 'exact' | 'prefix' | 'acronym' | 'fuzzy' | 'contains' | 'fts';
    ranges?: Array<{ start: number; end: number }>;
  }>;
}

export interface CommandPaletteEntry {
  id: string;
  kind: SearchVertical;
  label: string;
  subtitle?: string;
  icon?: React.ReactNode;
  action: PaletteAction;
}

export type PaletteAction =
  | { kind: 'command'; commandId: string }
  | { kind: 'apply-query'; query: string }
  | { kind: 'open-source-mailbox'; sourceId: string; mailboxId: string; name: string }
  | { kind: 'open-smart-mailbox'; smartMailboxId: string; name: string }
  | { kind: 'open-message'; sourceId: string; messageId: string; conversationId: string; mailboxHint?: { mailboxId: string; name: string } }
  | { kind: 'open-settings'; category?: SettingsSurfaceCategory }
  | { kind: 'open-compose'; intent: ComposeIntent }
  | { kind: 'open-contact'; contactId: string }
  | { kind: 'noop'; label: string };
```

```ts
export interface RankingContext {
  now: number;
  app: {
    route: 'inbox' | 'mailbox' | 'thread' | 'search' | 'composer' | 'settings';
    accountId?: string;
    mailboxId?: string;
    selectedMessageId?: string;
    selectedThreadId?: string;
    selectedContactId?: string;
    composerState?: 'none' | 'new' | 'reply' | 'forward';
  };
  session: {
    paletteOpenReason: 'keyboard' | 'button' | 'command-chain';
    previousPaletteQuery?: string;
    lastActionId?: string;
  };
  user: {
    recentCommands: DecayedCounter;
    recentEntities: DecayedCounter;
    frequentCommands: DecayedCounter;
    frequentMailboxes: DecayedCounter;
    frequentContacts: DecayedCounter;
    pinnedCommands: string[];
    pinnedMailboxes: string[];
  };
  model?: LocalRankingModelSnapshot;
}

export interface DecayedCounter {
  halfLifeMs: number;
  entries: Record<string, { value: number; updatedAt: number }>;
}

export interface LocalRankingModelSnapshot {
  version: string;
  featureWeights: Record<string, number>;
}
```

```ts
export interface ProviderState {
  status: 'idle' | 'loading' | 'done' | 'error';
  candidates: SearchCandidate[];
  nextCursor: string | null;
  error?: unknown;
  latencyMs?: number;
  indexVersion?: string;
}

export interface CommandSearchSession {
  query: string;
  queryVersion: number;
  context: RankingContext;
  providerStates: Map<string, ProviderState>;
  rows: PaletteRow[];
  selectedCandidateId: string | null;
  isLoading: boolean;
}

export interface CommandSearchController {
  session: CommandSearchSession;
  setQuery(query: string): void;
  loadMore(providerId: string): void;
  cancel(): void;
  select(candidateId: string | null): void;
  execute(candidateId: string): void;
}
```

```ts
export interface PaletteImpressionEvent {
  eventId: string;
  queryShape: {
    length: number;
    tokenCount: number;
    hasEmailAddress: boolean;
    prefixes: string[];
  };
  context: RedactedContextFeatures;
  shown: Array<{
    candidateId: string;
    vertical: SearchVertical;
    rank: number;
    features: Record<string, number | boolean>;
  }>;
  selectedCandidateId?: string;
  dismissed: boolean;
  providerLatenciesMs: Record<string, number>;
  staleSearchCount: number;
  cancelledSearchCount: number;
  timeToSelectionMs?: number;
  timestamp: number;
}
```

Raw query text may be kept only in a local, explicitly scoped replay store if needed for ranking development. Do not emit raw email addresses or message content into normal INFO-level logs.

## 5. Data flow

### Empty query

1. Palette opens and creates a search session with query `''`.
2. Coordinator captures context immediately.
3. Command and query-help providers return synchronous candidates.
4. Mailbox/tag providers search local read-model caches if ready; if `/read` is still loading, their states are `loading` or `idle` without blocking commands.
5. Message provider is not called against the backend for empty query. Recents come from local history and the messages already in the React Query cache (see §3 *Empty query shows recents*).
6. Ranker heavily weights context: selected thread actions, frequent commands, pinned mailboxes, current mailbox actions.
7. UI renders Best matches and available sections.

### Short query

1. User types one or two characters.
2. Coordinator increments `queryVersion`, aborts prior provider requests, and starts a new bounded search.
3. Local providers return prefix/acronym/fuzzy matches.
4. Message provider calls the backend only once the free-text query clears the minimum length (2-3 chars) or query validation says a query-language query is complete. Incomplete prefixes such as `from:` stay local and drive completions only (see §3 *Minimum query length*).
5. Context is moderate; exact/prefix textual matches beat unrelated contextual guesses.

### Message query

1. User types a complete valid message query such as `from:alice report`.
2. Query validation normalizes the query for backend execution.
3. MessageSearchProvider calls the shared `MessagePageClient` with `limit` around 8-25 and operation kind `mail.search.preview`.
4. Current implementation maps to `/v1/messages/search` (`sort=date`); the end state maps to the unified `/messages/search` with `sort=relevance`.
5. Backend returns a cursor page. Current backend order is sort-based; future preview order is relevance-based over FTS results.
6. Provider wraps each `MessageSummary` as a message candidate with field evidence and mailbox hints.
7. Coordinator blends message candidates with local candidates.

### Scroll pagination

1. Palette row virtualizer reports that the viewport is near the end of a provider section or the total flattened list.
2. Coordinator chooses providers with `nextCursor`, usually the visible provider sections first. It does not ask every provider to page unless that behavior is explicitly desired.
3. Provider receives `cursor` and bounded `limit`.
4. New candidates append to that provider reservoir.
5. Ranker may recompute loaded candidate scores, but UI selection stays pinned by candidate ID. Best Matches should not churn below the user’s cursor.

### Stale request cancellation

1. Every query change increments `queryVersion`.
2. The current `AbortController` aborts in-flight provider requests.
3. Provider responses include the version captured at dispatch time.
4. Coordinator applies a response only if the version still matches and the request was not aborted.
5. Late responses are counted for diagnostics and ignored.
6. Palette close aborts all in-flight provider requests and writes dismissal/impression data.

## 6. Implementation sequence

The original Phase 0 bundled a large, independent startup refactor with the palette cleanup. They are separated here: the startup unblock is its own spec (see *Startup unblock — separate spec* below) and is no longer a prerequisite. The palette work is sequenced so the early, low-risk phases deliver most of the value, and the riskier machinery (full ranking, FTS, startup refactor) comes later, gated on evidence.

### Phase 1: cleanup and shared message page client

- Remove `useCommandPaletteResults` as the authoritative search implementation; keep only small row-rendering helpers if useful.
- Remove contacts derived from `cachedMessages`, and the `from:`/`tag:` completions scraped from the search-preview page (`queryLanguage.ts` `candidatesForPrefix`). Replace with no contact provider, or a clearly bounded local correspondent provider only if it has a real source.
- Remove UI-only caps as architectural controls. Preserve visible limits only as rendering defaults fed by provider limits.
- Add command-search types under a focused module, for example `apps/web/src/command-search/types.ts`.
- Add `MessagePageClient` around `fetchSourceMessages`, `fetchSmartMailboxMessages`, and `fetchSearchMessages`, and route `MessageList` through it while keeping its current `useInfiniteQuery` behavior.
- Keep the current backend-bounded message search call as the temporary message provider backend.

### Phase 2: coordinator, local providers, ranker, and rows

- Implement `CommandProvider` (from the current static command list), `QueryCompletionProvider` (wrapping `queryLanguage.ts`), `MailboxProvider`, and `TagProvider`. Add `ContactProvider` only if it has a bounded source; otherwise leave contacts out rather than faking them.
- Implement `MessageSearchProvider` over the shared `MessagePageClient`, with the minimum-query-length rule and empty-query recents from cache.
- Add `SearchCoordinator` and `useCommandSearch()`: query versioning, abort handling, provider states, cursors, late-response ignoring, `loadMore`, and the settlement contract from §2.
- Implement the deterministic lexicographic ranker and the hybrid row builder (Best Matches plus sections, deduped).
- Render coordinator rows through the existing `cmdk` primitives (no virtualization). Preserve keyboard semantics; skip non-`item` rows in navigation.
- Add scroll-near-end `loadMore` for visible provider sections, with selection pinned by candidate ID.

This phase alone replaces today's palette with a bounded, multi-provider, relevance-aware one.

### Phase 3: instrumentation and replay

- Add typed events to `LOG_EVENTS` only for low-cardinality operational events such as provider latency, cancellation, stale response, and execution outcome.
- Add a local `PaletteEventSink` for impression and selection events with redacted query/context by default.
- Track provider latency, cancelled/stale searches, query length, time to selection, selected vertical/action, and dismissal.
- Defer `DecayedCounter` persistence and ranker replay tests until there is evidence the ranking needs them; capture the feature vectors first so the data exists when that decision is made.

### Phase 4: unified FTS backend

- Make `/messages/search` relevance-capable via `sort=relevance|date`, backed by a SQLite FTS5 index. Introduce a `/messages/search-preview` sibling only if needed as a temporary staging endpoint, and plan its retirement.
- Design the FTS schema up front for the tokenizer, prefix, index-sync, and per-account scoping issues in §2 *Backend message search*.
- Add OpenAPI/types and authz coverage, including the data-scoping requirement.
- Update `crates/posthaste-domain/src/search.rs` operator mappings and the `L1-search` spec; add tests comparing FTS results against the documented operator behavior.
- Point `MessageSearchProvider` at `sort=relevance` once the index is ready.

### Startup unblock — separate spec

Today `App.tsx` early-returns a full-page "Setting up…" screen while `accountsQuery`/`mailNavigationBootstrap` load (`App.tsx:459`), so the palette cannot open before `/read` finishes. Making the shell render with absent read models touches `App.tsx`, `Sidebar`, `MessageList`, and `ActionBar`, each of which currently assumes read-model data is present. That is a meaningful refactor with its own regression surface and only a thin payoff for the palette (before `/read`, only commands and query help are available anyway).

Track it as its own spec, sequence it independently, and justify it on its own merits (shell responsiveness) rather than as a palette prerequisite. The palette architecture above does not depend on it: it already tolerates providers that are `loading`/`idle`/`unavailable` without blocking. If and when the shell unblocks, the palette gains earlier mailbox/tag/message availability with no architectural change.

## 7. Acceptance criteria

- Command palette opening *before* `/read` finishes is handled by the separate startup-unblock spec. Here, the palette must not *block* on read models: it shows commands and query help immediately and adds other providers as they become ready.
- Built-in commands and query help are searchable immediately.
- Mailboxes, tags, and messages appear as their providers become ready; they do not block the palette.
- The client never loads all messages across mailboxes for palette search.
- The client never scans or ranks all local messages on scroll.
- Provider requests are cancellable with `AbortController`.
- Responses from old query versions are ignored.
- Message provider retrieval is backend-owned, bounded, and cursor-paginated.
- Scrolling requests provider next pages; it does not reveal hidden preloaded global results.
- Loaded candidates may be reranked; the global message corpus is not reranked client-side.
- Best Matches plus vertical sections are rendered, with duplicate actionable rows removed from lower sections.
- Keyboard navigation remains stable when provider pages arrive.
- Provisional message-list preview still clears on close/reject and applies only valid backend queries.
- MessageList and command palette share message page API retrieval code, not UI state.
- Startup responsiveness does not regress.
- Instrumentation records provider latency, stale/cancelled searches, impressions, selections, dismissals, and time to selection without logging message bodies or secrets.
- An empty query shows recents from local history and cached messages, without issuing a backend message request.
- The message provider does not issue a free-text backend request below the minimum query length.
- Best Matches reorders only until the query version settles (all selected providers returned or the 250 ms deadline), then freezes; selection stays pinned by candidate ID and rows above it do not move.
- Non-`item` rows (section/loading/empty/error) are skipped by keyboard navigation and ignored by Enter.
- The message list and palette use one backend search engine; they do not run divergent retrieval over the same query language.
- Search results never cross account-authorization boundaries.
- `CommandProvider` ships against the static command list; the contextual-action registry is not a build dependency.
- Latency targets are met: first row from local providers renders within one frame (~16 ms) of a keystroke, and message-provider p95 is recorded against a baseline captured before Phase 2.

## 8. Risks and tradeoffs

### Performance

The provider/coordinator split adds more moving parts, but it keeps expensive work in the right place. Local providers remain small. Message retrieval stays backend-paginated. The largest risk is accidental reintroduction of client scans through contacts or local message completions; tests and code boundaries should forbid this.

### Ranking quality

Deterministic ranking will be worse than a mature learned model at first. That is acceptable. The priority is to produce stable feature logs and replayable sessions so ranking changes can be evaluated. A premature model without retrieval boundaries would make correctness and privacy worse.

### UI jumpiness

Hybrid ranking can move rows when providers finish at different times. The settlement contract (§2) bounds this: Best Matches reorders only until the query version settles or the 250 ms deadline, selection is pinned by candidate ID, freezing is anchored to the user's selection, and later pages append inside sections. The residual risk is choosing the deadline — too short locks out a slightly slower message provider, too long lets rows churn. Make it a tunable constant and validate it against measured provider latencies.

### Backend dependency

Good message navigation depends on backend relevance search. The current `/messages/search` endpoint gives a safe bounded bridge but not final Raycast-quality relevance. FTS5 should be treated as a required follow-up for high-quality message results.

### FTS migration

Switching from `LIKE` to FTS5 changes recall and per-operator semantics, not just performance. Email-address tokenization, prefix matching, and external-content index sync are the specific footguns (§2). Treat it as a query-semantics migration: pin operator-to-match-strategy decisions in `search.rs` and `L1-search`, and add tests comparing FTS results against the documented operator behavior before switching the provider over.

### Startup unblock scope

Unblocking the shell before `/read` is a cross-cutting refactor (`App.tsx`/`Sidebar`/`MessageList`/`ActionBar`) carved out into its own spec. The risk is scope creep back into the palette work; keep them separate and do not gate the palette on it.

### Spec divergence

This plan changes the startup invariant and the message-search backend, which will diverge from `docs/L1-ui`, `docs/L1-api`, and `docs/L1-search`. Update those specs as each phase lands: `L1-api` for the `sort=relevance` parameter and any staging endpoint, `L1-search` for the FTS operator semantics, and `L1-ui` for the palette result model and the (separately specced) startup behavior. Keep `modified`/`reviewed` current per the staleness rules.

### Privacy and local history

Personalization needs local history. Keep counters local, bounded, and inspectable. Do not log raw bodies. Treat raw queries as sensitive because they may contain email addresses or private terms. Use redacted query shapes for normal instrumentation and reserve raw-query replay for explicit local debugging only.

### DRY boundary

Sharing too much with MessageList would couple two different products: a sorted mailbox table and a command palette search session. Share the message-page client, query normalization, cursor mechanics, generated API types, and operation correlation. Keep ranking, provider state, selection stability, and row rendering separate.
