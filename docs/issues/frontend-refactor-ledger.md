---
title: "Frontend refactor ledger — post-retirement code quality worklist"
modified: 2026-07-19
state: resolved
---

# Frontend refactor ledger

Ranked survey of apps/client/frontend (2026-07-18, ~41k LOC excluding gen/).
Baseline health: zero `as any`, zero ts-suppressions, files ≤712 LOC — the
debt is structural duplication, not type fragility.

ALL ITEMS RETIRED 2026-07-19 — the ledger closed with charter slice 5
(docs/client/L2-charter.md, §Execution order).

1. **Unify the three action layers.** — RETIRED 2026-07-19 (charter slice 4,
   R4). One command table: `commands/registry.ts` + `commands/defs/{global,
   navigation,mail,compose,shared}.ts`, each entry
   `{ id, chord?, availability(ctx), run, surface }`, consumed by palette,
   keyboard dispatcher, hover actions, and menus alike. `useEmailActions`
   survives only as the execute/toast layer under the table (per-account undo
   routing lives there); availability predicates exist once, in the defs.

2. **Type the event payloads in the models crate.** — RETIRED 2026-07-19.
   Payload variants are generated TS (`gen/MessageUpdatedPayload`,
   `gen/SyncCompletedPayload`) and `DomainEventKind` is a closed union pinned
   to the emitted set by a drift test. One boundary parser —
   `data/transport/stream.parseMessageUpdated` — replaces per-consumer shape
   sniffing; `newMailArrivals.ts` consumes the contract (290 LOC, defensive
   branches and the dead branch gone).

3. **`useComposeFormState` state machine.** — RETIRED 2026-07-19 (charter
   slice 4). Now `compose/form/model.ts` (pure reply/forward-context
   derivation) + `compose/form/machine.ts` (one reducer with named events),
   each with its own test file; `useComposeFormState` is the thin React
   binding. The send/undo bug (docs/issues/integrated-send-undo-broken.md)
   was fixed behind it as planned.

4. **Settings form ↔ FieldPatch glue.** — RETIRED 2026-07-19 (charter
   slice 5). `components/settings/forms/fields.ts` owns declarative
   `FormField` descriptors — `{ name, read, dirtyCompare, toPatch }` — shared
   by `accountForms.ts` and the editors; patch assembly built on them never
   restates stored state (an untouched field always yields
   `{ kind: 'keep' }`). Tested in `fields.test.ts`.

5. **Split `client.ts` (658 LOC).** — RETIRED 2026-07-19. MailClient now
   composes two independently tested units: `transport/http.ts` (fetch/auth,
   error mapping, token-in-URL rules) and `transport/eventStream.ts` (SSE
   lifecycle, reconnect backoff, run-id/generation tracking, prompt
   dispatch); the facade keeps the wire shapes and multi-step verbs. The
   refcounted watch registry was not split but DELETED: react-query became
   the one mirror when the queries moved to it, and no consumer retained a
   watch anymore — the registry, its debounce, and the duplicate verb set
   (markRead/flag/move, re-owned by `transport/commands.ts`) were dead
   weight (tenet XIV).

6. **Collapse `useMailListPages`.** — RETIRED 2026-07-19 (the hook had
   become `useMailListView` over `useInfiniteQuery`; invalidation still
   refetched every accumulated page). Now ONE windowed query per view:
   `fetchMailListWindow` fills `pages × MESSAGE_PAGE_SIZE` rows from the top
   in server-capped chunks (the backend clamps `limit` to MAX_LIST_LIMIT and
   hands back cursors, so the cap is never restated client-side), and the
   window depth persists per view like scroll offsets. An invalidation
   refetches the deep-scrolled list exactly once — asserted by
   `model.test.ts` ("O(1) in pages"); rows stay live, the scroll prefix
   stable, and window growth rides `keepPreviousData`.

Non-issues checked and cleared: switch-over-kind sites are closed generated
enums (exhaustiveness-guarded); `conditionValueWidgets.tsx` is large but
parallel JSX, not logic sprawl.

Suggested order was 2 → 1+3 together (the two known bugs lived there) →
4–6 opportunistically; the charter slices absorbed them in that shape.
