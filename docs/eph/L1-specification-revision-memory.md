---
scope: L1
summary: "Ephemeral working memory for Posthaste specification revision feedback and design corrections"
modified: 2026-06-15
reviewed: 2026-06-15
lifecycle: ephemeral
type: NOTES
depends:
  - path: docs/api/L1
  - path: docs/backend/L1
---

# Specification revision memory

This file records designer feedback during the specification revision workspace. Use it as working memory before drafting or revising specs, so repeated mistakes are not reintroduced.

## Active feedback

### L0 architecture draft style and framing — 2026-06-14

- Use numbered section headings (`1.`, `1.1`, `2.`, etc.) for this architecture spec.
- Do not include a first `# Architectural spine -- L0` heading when the file title/scope already provides that context.
- Avoid meta-explanation about SPECial levels or where other details belong. The file should be about Posthaste architecture, not about what the spec file is doing.
- Prefer simpler direct statements. In particular, avoid self-obvious sentences like “This document defines Posthaste's architecture.”
- Use concise heading names when the topic is already clear from the filename. Example: `Thesis`, not `Architectural thesis`.
- Avoid defining concepts by negation unless the contrast is essential. Removed phrases like “not separate privileged paths into mail state.”
- Current preferred opening concept: “Posthaste is a mail workstation built around a single-user, stateless backend authority.” Note: `stateless` is accurate for the current framing but may eventually stop being true.
- Do not include a separate `Non-goals` section in this L0 architecture draft.
- Avoid L1/L2-level ownership lists in L0 architecture. A sentence like “the backend owns provider access, sync, persistence, secrets, sanitization, generated contracts, and events” is too contractual and belongs in backend/API/security/event docs.
- Avoid implementation/client details in L0 architecture. Terms like “human client,” “React,” “Tauri,” and desktop embedding details are too specific for L0 architecture unless the domain is specifically client/desktop.
- L0 architecture should only help an agent decide whether deeper architecture docs are relevant. If it cannot do that better than domain L0 docs plus the root, reconsider whether `L0-architecture` should exist at all.
- Open design question: architecture may belong in the root/domain map rather than as a separate domain. Candidate split: backend, API, client, desktop/Tauri distribution, MCP, and other distribution modes as separate domains.
- Current direction: pause `L0-architecture`. Do not continue drafting L0s by default; L0 is only useful for non-obvious domains. Start with L1/L2 for concrete domains, beginning with backend.
- API should be a separate domain from backend internals. A client-facing agent needs API contracts without needing backend internals.
- Backend docs may still include API-related implementation details where relevant, such as how an API path is routed/resolved internally.
- Prefer SPECial section/local dependencies for overlap. If one file derives a section from another, use `depends` entries with `path`, `section`, and `local` rather than duplicating untracked claims.
- Current preference: API contract should likely go first, and backend docs should specify how backend implements that API, rather than API being derivative from backend internals.
- Backend L1 should use a taller numbered section tree. Include motivation subsections where a contract represents a real design choice, so future agents see the reasoning and the drafting process forces explicit design review.
- Motivation subsections must sit below the section they motivate, not as siblings at the same level. If motivation explains `1.1 Contract`, use a child like `1.1.1 Motivation`, not a sibling like `1.2 Motivation`.
- Removed from backend L1: “A change to one side should not require unrelated layers to import its private data model.” This is a broader architectural/client rule, not specifically backend. It should be expanded elsewhere rather than left as a throwaway backend sentence.
- Current process preference: get L1/L2 specs running and workable before perfecting wording. Move forward to backend L2 even if backend L1 still needs edits.
- API is a client-facing domain. L1 API should document external endpoint/auth/error/event contracts without handler names; handler/router/authz implementation belongs in L2 API.
- Defer API L3 for now. Add it later only for implementation traps and debugging details such as authz caveat edge cases, OpenAPI drift workflow failures, OAuth callback edge cases, or SSE replay bugs.
- Do not preserve old `@spec docs/...#anchor` references by adding compatibility HTML anchors just to avoid breakage during spec revision. It is acceptable to break those references temporarily and restore/update them later if needed.
- API direction to integrate: HTTP endpoints and SSE state assertions should coexist. SSE should move from broad invalidation toward typed post-state assertions, collapsed catch-up, and snapshots. HTTP read endpoints may stop being the web client's primary state path but should remain for stateless clients such as MCP, scripts, tests, and debugging.
- API spec should ask what base representation HTTP and SSE share, so they do not drift. Prefer defining canonical resource/state representations and making HTTP snapshots/lists and SSE assertion fields derive from those representations where possible.
- Do not update domain docs that have not been redone yet just to propagate the new API direction. Leave stale dependent docs alone until their rewrite pass; update only the docs currently in scope.
- Directory convention started 2026-06-14: current rewritten domains live in per-domain subdirectories such as `docs/api/L1.md`, `docs/backend/L2.md`, and nested domains such as `docs/state/mail/L1.md`. Unrewritten legacy specs live under `docs/stale/` until their domain rewrite pass.
- Mail state ownership belongs in `docs/state/mail/`: canonical messages/mailboxes/keywords/bodies, derived conversations/query views, backend-side query evaluation, conversation refs/tokens, and runtime view rules.
- UI renderer behavior belongs in `docs/client/`: presentation state, runtime adapter, view subscriptions, and named action dispatch. The UI is a renderer over runtime state, not a mail cache authority.
- Avoid just-in-case optional event types. Conversation projection assertions are not part of the message-change design; message before/after summaries carry `ConversationRef`, and the runtime refreshes stale visible conversations.
- Query authority is the full `QueryScope`, not per-atom renderer state. Do not model query-scope invalidation events; the backend emits state assertions such as message, mailbox, smart-mailbox, or account before/after changes, and the runtime derives active view recomputation from those facts.
- Draft changes are ordinary message changes. Message summary state carries `bodyToken` and `attachmentToken`; detail/body or attachment changes are represented by changed tokens, not draft-specific events or body payloads in SSE.
- Bundled application mode is modeled as packaged renderer + embedded authority runtime. It must not require `posthaste serve` or a hosted backend for ordinary use. Current embedded-loopback HTTP can be a migration bridge only if renderer components target the runtime adapter facade.
- Bundled application mode is UI renderer + embedded authority runtime. The deferred architecture question in `docs/eph/TODO-L1-specification-revision.md` is remote/offline local replica design, not bundled-mode stateful views.
- Runtime adapter interface direction: components open serializable view descriptors, receive full view snapshots/replacements, submit named mutations with `ClientMutationId`, and render runtime settlement frames. Avoid row-patch/client-repair language unless the runtime still owns full replacement snapshots.
- Mail-list window direction: use `MailListViewState` and `MailListRowState`; rows carry stable row keys and opaque ordering metadata. UI owns scroll anchors/virtualizer state and asks the runtime for windows around cursors/anchors; it never synthesizes rows to preserve scroll.
- Runtime coverage/idempotency fixes from review: keep lifecycle separate from future replica coverage; reserve `coverage` plus `readWatermark` on view snapshots. Renderer passes them through but does not interpret freshness. Persist accepted `ClientMutationId` mappings in runtime state before local effects/provider commands. Conflict choices return as named mutations referencing the original conflict.
- Named mutation catalog direction: renderer submits user intent (`message.setReadState`, `message.moveToRole`, `draft.save`, `message.send`, `settings.patch`, `account.patch`, etc.). Runtime owns role mailbox resolution, keyword normalization, local before/after state, provider queue/submission, idempotency, undo/retry/conflict support, and settlement.
- Runtime crate direction: the UI/API-facing runtime contract should live outside `posthaste-server` and outside the authority implementation. The bundled app embeds a reusable authority runtime crate. Future hosted/offline modes can use a different local-replica runtime implementation behind the same contract.
- Authority runtime handle direction: build one transport-free `RuntimeHandle` before adapters. Axum and Tauri call the shared runtime contract implemented by that handle; handle methods accept domain/runtime inputs plus adapter-neutral caller context, not HTTP/Tauri/React types. Shutdown belongs to the runtime handle.
- Bundled storage/security direction: runtime owns config/state/cache roots and provider secrets; renderer storage is presentation/profile metadata only. Loopback tokens are local, non-URL capabilities. Tauri exposes named commands only, validates input strictly, blocks untrusted navigation, and opens only allowed external URL schemes.
- Implementation test direction: start with a runtime-contract/authority-runtime crate test that builds `RuntimeHandle` without HTTP/Tauri, keep API contract tests passing through the handle-backed router, test renderer hooks against the runtime adapter facade, and prove send idempotency by asserting reused `ClientMutationId` does not submit twice to the mock provider.
- Reference models now used in current specs: JMAP for opaque state/change recovery, OpenAPI/AsyncAPI/SSE for API contracts, TanStack Query/SWR for query/state references only, Relay/Apollo for normalized entity/query cache shape references, SQLite query planner docs for backend query optimization, and Replicache/Electric/PowerSync/RxDB/WatermelonDB as deferred local-replica prior art.

## How to use this file

When the designer explains an edit or rejects wording, add a short note with:

- the spec area affected
- what was wrong
- the preferred framing
- any exact language or term to preserve

Keep entries concise. This is not a decision log; it is a scratchpad for avoiding repeated drafting errors during this workspace.
