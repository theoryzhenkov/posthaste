---
title: "Frontend refactor ledger — post-retirement code quality worklist"
modified: 2026-07-18
state: open
---

# Frontend refactor ledger

Ranked survey of apps/client/frontend (2026-07-18, ~41k LOC excluding gen/).
Baseline health: zero `as any`, zero ts-suppressions, files ≤712 LOC — the
debt is structural duplication, not type fragility.

1. **Unify the three action layers.** The same verb semantics live in
   `actions/defs/message.ts` (palette/keyboard registry + availability
   predicates), `hooks/useEmailActions.ts` (its own role dispatch + toast
   policy), and `data/commands.ts` (raw verbs). Availability logic is already
   drifting between registry and hook. Target: one action table —
   { verb, availability(ctx), execute, toastPolicy } — consumed by palette,
   keyboard, hover actions, and menus alike.

2. **Type the event payloads in the models crate.**
   `DomainEventPayload.payload` crosses the wire untyped, so consumers sniff
   shapes defensively — `notifications/newMailArrivals.ts` (309 LOC, 20
   conditionals, one known-dead branch) is the worst case. Typing the payload
   variants (generated TS like the rest of the protocol) converts fragile
   checks into contract and shrinks every consumer.

3. **`useComposeFormState` is a state machine hiding in 8 useStates**
   (409 LOC, 29 conditionals: reply/forward derivation, from-account
   selection, attachment flags, reset keys). Target: pure reply-context
   derivation + one reducer with named events. Do this BEFORE fixing the
   send bug (docs/issues/integrated-send-undo-broken.md) — it is send's
   neighborhood.

4. **Settings form ↔ FieldPatch glue, hand-rolled per form.**
   `accountForms.ts` (338 LOC) plus AccountEditor/ConnectionEditor/TagsPane
   each build keep/set/clear patches by hand. Target: declarative field
   descriptors (name, read, dirty-compare, to-patch) shared by all editors.

5. **Split `client.ts` (658 LOC).** MailClient carries transport, stream
   lifecycle, run-id/reconnect, the refcounted watch registry, and refetch
   scheduling in one class. Target: connection state machine and watch
   registry as separate, independently tested units.

6. **Collapse `useMailListPages`.** One live query per accumulated scroll
   page means every mutation refetches 11–13 queries (measured; grows with
   scroll depth). Target: a single windowed query on mutation-invalidate.

Non-issues checked and cleared: switch-over-kind sites are closed generated
enums (exhaustiveness-guarded); `conditionValueWidgets.tsx` is large but
parallel JSX, not logic sprawl.

Suggested order: 2 → 1+3 together (the two known bugs live there) → 4–6
opportunistically.
