---
title: "Client charter (L2)"
scope: L2
summary: "The frontend constitution: the rules that generate refactoring decisions, the target directory shape, the owned vocabularies, the branded primitives, and the ratchet that keeps entropy from returning."
modified: 2026-07-18
reviewed: 2026-07-18
state: draft
depends:
  - path: docs/client/L1-client
dependents:
---

# Client charter

This document scales one person's judgment: the owner ratifies these rules
once; agents and humans apply them per-instance and emit one-line decision
logs; only genuinely novel questions escalate (batched, max five per slice).
Every rule below is a DEFAULT — it stands unless struck.

Foundation: the tenets at <https://shapeofthesystem.com> are adopted wholesale.
The ones that bind hardest here: I (locality of reasoning), II (explicit data
flow), III (parse, don't validate), VIII (tear down what you set up),
XIII (visible failure modes), XIV (one source of truth), XXI (simplicity is
the budget), XXII (name to reveal). Rules below are their frontend
instantiation plus the house-specific additions.

## The rules

- **R0 — Incumbents win.** When a pattern already has a best instance in the
  tree (`domainVocabulary.ts`, the actions registry, `gen/`), consolidate
  onto it. New designs need a reason the incumbent can't serve.
- **R1 — Rule of two.** A helper, constant, or type used by one module stays
  local. The second consumer moves it to its responsible module in the same
  change. Nothing moves "for later" (tenet XXI).
- **R2 — Vocabulary is owned once.** A closed string set used in two or more
  files lives in `domain/vocabulary.ts` as a const object + derived union
  (`Status.error`, `type Status = ...`). A set that originates on the wire is
  imported from `gen/` and never restated (tenet XIV). Known instances to
  consolidate: `QueryStatus`, `ConnectionStatus` (client.ts), mailbox-role
  sets, severity/tag/window vocabularies flagged by the census.
- **R3 — Parse, don't validate.** No exported `is*(raw: string): boolean`
  validators. Unstructured input becomes a branded type at the boundary via
  `parseX(raw): X | null`; interior signatures take the branded type and
  never re-check (tenet III). Initial branded set: `EmailAddress`,
  `EmailPattern` (the `isConcreteEmailAddress`/`isConcreteEmailPattern`
  duplicate pair collapses into these), `MailboxRole`, `Chord`,
  `IsoDateTime`, `SearchQuery`, plus thin brands over the `gen/` id aliases
  (`AccountId`, `MessageId`, ...) at the data-layer boundary.
- **R4 — All input is a command.** Keyboard, palette, context menu, and
  hover actions route through the command registry — one entry:
  `{ id, chord?, availability(ctx), run, surface }` — organized as
  `commands/{global,navigation,mail,compose}.ts`. No component calls
  `window.addEventListener` for input; the allowlist is the registry's
  dispatcher plus named low-level UI primitives (focus trap, resize
  observer). The devtools shortcut in App.tsx is the first migration.
- **R5 — One store implementation.** `lib/store.ts` owns the
  subscribe/getSnapshot/listeners pattern once (`createStore<T>()`); the five
  hand-rolled copies (client-preferences, onboarding, column config, view
  mode, ...) migrate to it. New client-local state uses it or React state —
  nothing hand-rolls subscription plumbing.
- **R6 — No `utils`.** Shared helpers live in `lib/<domain>.ts` named by
  what they operate on (`lib/dom.ts`, `lib/format.ts`, `lib/list.ts`); the
  existing `utils/` dissolves. A file named `helpers`/`utils`/`misc` is a
  lint error (tenet XXII: names reveal responsibility).
- **R7 — Effects tear down.** Every listener/observer/timer registered in an
  effect returns its cleanup (tenet VIII). Already the norm; stated so the
  ratchet can enforce it on new code.
- **R8 — Ambient dependencies have seams.** Clock, randomness, storage, and
  the network cross one injectable boundary each (the facade already does
  this for fetch/EventSource; `Date.now()`/`localStorage` calls scattered in
  components migrate behind `lib/time.ts` / the preferences store)
  (tenet II).
- **R9 — Naming.** Modules are nouns owning a domain (`address.ts`, not
  `composeAddressSuggestions.ts` for a generic email predicate); hooks are
  `useX`; commands are `verbNoun`. A name states the fact the type cannot:
  units, side effects, ordering (tenet XXII).
- **R10 — Folder shape: 3–8 entries.** Every folder holds at most 8 entries
  (files + subfolders combined) and, except leaves, at least 3 — deeper and
  balanced beats wide and flat. Tests colocate with their source and do not
  count toward the cap. Restructures preserve git history (pure moves,
  separated from logic changes).
- **R11 — Import boundaries.** A module imports only from: its own subtree,
  `lib/`, `domain/`, `data/`, `gen/`. `app/` is the sole composition root
  (the only place allowed to import everything). `components/` never imports
  `commands/` (commands bind UI to verbs, not the reverse). Enforced by
  dependency-cruiser (tenet I: locality — a reader of a module sees its
  world in its imports).
- **R12 — Decision logs over approvals.** Every consolidation slice emits
  one line per instance decided (`isConcreteEmailPattern ->
  domain/address.parseEmailPattern, 6 call sites`). The owner skims and
  vetoes; silence is consent. Ambiguity without an incumbent escalates in
  batches of ≤5 questions.

## Target directory shape

Top level of `src/` (today: 68 entries) becomes:

```
src/
  app/         composition root: main, App, providers, shell, layering
  commands/    registry + dispatcher; defs: global, navigation, mail, compose
  components/  pure UI, ≤8 per folder: mail/{list,thread,detail}, compose/,
               settings/{accounts,rules,tags,appearance}, sidebar/, ui/,
               floating/
  data/        the facade split: transport, stream, queries, verbs, hooks
  domain/      vocabulary.ts + branded types/parsers: address, role, time,
               search
  desktop/     tauri glue: updates, devtools, repair, windows, diagnostics
  lib/         domain-free: store, dom, format, list
  surfaces/    surface routing, history, window policy
  gen/         generated wire types (never hand-touched)
```

The ~45 loose root files distribute per R9/R11 (each move logged, not
pre-approved here). `components/settings-panel` (45 entries) and
`compose-overlay` (20) subdivide per R10. Moves land as a dedicated
moves-only slice — pure renames + import fixups, zero logic change, so the
diff is verifiable by shape alone.

## The ratchet

Entropy returns unless the rules are mechanical. Landed with the first
slice, all fail-on-NEW-only against committed baselines:

- dependency-cruiser: R11 boundaries + the R4 addEventListener allowlist.
- ESLint no-restricted-syntax: exported boolean validators over raw strings
  (R3), `utils`/`helpers` filenames (R6), hand-rolled store plumbing (R5).
- knip: dead exports/files (delete beats organize).
- jscpd: copy-paste budget, current level as ceiling.
- folder-shape check (small script): the R10 3–8 rule.

## Execution order

1. **Moves-only slice** — the target tree, pure renames (R10), history
   preserved. Everything after happens in the new geography.
2. **Ratchet slice** — the tooling above, baselined.
3. **Vocabulary slice** (R2), then **primitives slice** (R3) — these shrink
   every later diff.
4. **Commands slice** (R4) and **store slice** (R5) — absorbs refactor-ledger
   item 1; compose/send work (ledger 3 + the send/undo bugs) rides behind it.
5. **Helper/constant sweep** (R1, R6, R8, R9) — the long tail, largely
   mechanical against the ratchet.

Each slice: green gate (tsc, tests, build), decision log in the commit
message, baseline shrinks or holds. The refactor ledger
(docs/issues/frontend-refactor-ledger.md) items map onto slices 3–5 and are
retired as they land.
