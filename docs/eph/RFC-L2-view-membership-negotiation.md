# RFC-L2: View-membership negotiation — one predicate, two evaluators, runtime-owned assignment

Status: **SUPERSEDED, pre-implementation** (2026-07-16, same day as drafted) —
by [RFC-L2-mirror-client], which removes the second evaluator instead of
negotiating with it, making the membership-assignment problem moot. Slice 0
(the testkit `MailListMirror`) LANDED and remains the L3 convergence-test
bridge until mirror-client Slice 3 deletes the machinery it mirrors. Slices
1–5 will not be implemented. §1 (the problem statement) doubles as evidence
for the mirror-client RFC. Original parent: RFC-L2-client-resilience;
superseded the "negotiate the mode" step of
docs/issues/L2-single-source-view-membership (option iii, step 4).

## 1. Problem

Option iii retired the per-event mail-list re-serve (the O(views × rows)
`build_snapshot` storm that dominated sync cost, and the dual-source membership
smell behind the move/delete flicker). What shipped as its control mechanism is
a **client-stamped boolean**: the web client derives "can I self-maintain this
view?" in `resolveMailListPredicate` and stamps `client_self_maintained` on the
`ViewDescriptor`; the runtime trusts the stamp and skips re-serves
(`view_registry::spawn_event_pump`).

Three defects, all structural:

1. **Unverifiable promise.** The runtime cannot tell whether the client
   actually folds the `message.updated` firehose. A wrong stamp = permanent
   silent staleness with no backstop until a reconnect collapse. Realized in
   the wild: the testkit's `ViewWatch` stamped `true`, folded nothing, and the
   L3 convergence test sat red-but-soft for weeks
   (`twenty_injected_messages_converge_into_inbox_view`).
2. **Wrong data locality.** The client resolves evaluability from react-query
   caches (role→mailbox maps) that may be unhydrated — the flag is
   timing-dependent on sidebar load. The runtime owns the mailbox/role tables
   the decision actually needs.
3. **Duplicated predicate logic that can't scale.** The membership predicate
   exists twice (runtime SQL; client `ViewPredicate` + a hand-curated
   evaluability whitelist). Every user-created smart mailbox falls to
   `deferred` and silently reinstates the per-event re-serve cost option iii
   was built to kill.

## 2. Decisions

- **D176 — Membership assignment is runtime-owned.** The caller declares an
  evaluation capability ONCE (`RuntimeCallerCapabilities.membership_vocabulary:
  Option<u32>`, beside the existing `view_delta` opt-in); the runtime decides
  per view whether the client maintains membership, and says so explicitly in
  the open-view result. The client-stamped `client_self_maintained` descriptor
  flag is retired. Rationale: the decider must be the party that (a) owns the
  data the decision needs and (b) acts on the outcome (skipping re-serves).

- **D177 — The membership contract is data, not a flag.** The open-view result
  (and every served `ViewSnapshot` for the view) carries
  `membership: ClientMaintained { predicate } | RuntimeServed`. Vocabulary v1
  `predicate` = the existing projector vocabulary: `inMailboxes([...])` /
  `all`. The client feeds the received predicate straight into its
  `EntityStore` — `resolveMailListPredicate` + `buildMailListPredicateContext`
  are deleted; the two evaluators cannot drift because only one party ever
  derives the predicate.

- **D178 — Fail toward re-serve.** Any view whose predicate exceeds the
  caller's declared vocabulary — or any caller that declares none — gets
  `RuntimeServed`, i.e. today's deferred per-event re-serve. The failure
  direction flips from "silent staleness" (today) to "redundant recomputes":
  old clients, dumb clients, and test harnesses are correct by default and
  merely slower. Staleness stops being a reachable failure mode of the
  protocol.

- **D179 — One compiler, two backends (vocabulary v2).** The smart-mailbox
  rule AST (`MailQueryRule`, already compiled to SQL by
  `rule_compiler::compile_mail_query_rule`) grows a second backend: a
  projection-decidable predicate IR evaluated by the replica projector.
  "Evaluable" becomes a **compiler-proven property** — the IR backend succeeds
  iff every referenced field is carried on the `message.updated` projection
  (mailboxIds, keywords, from, subject, receivedAt, …); body-FTS and other
  non-projection conditions make the compile fail and the view stays
  `RuntimeServed`. User smart mailboxes self-maintain exactly when provable,
  automatically, forever.

## 3. End state

- **Open:** client opens a view (scope/sort as today), having declared
  `membership_vocabulary: Some(N)` at caller construction. Runtime compiles
  the scope: predicate fits vocabulary ≤ N → response carries
  `ClientMaintained { predicate }`; else `RuntimeServed`.
- **Events:** `ClientMaintained` views get NO per-event re-serve (option iii,
  unchanged); the client folds the firehose through its store using the
  received predicate. `RuntimeServed` views recompute + re-serve per affecting
  event (unchanged).
- **Lifecycle:** opens, pagination/window-extend, and resync/gap-recovery
  re-serves stay runtime-served for ALL views (a partial-window store cannot
  self-derive them) — unchanged from option iii.
- **Recovery:** reconnect-collapse `refresh_open_views` remains the staleness
  backstop of last resort (unchanged), but no longer load-bearing against
  protocol-level mislabeling — that class is gone by construction (D178).

## 4. Migration slices

- **Slice 0 — the client half in the testkit (LANDED with this RFC).**
  `ViewWatch` embeds the real `posthaste-replica-projector::EntityStore` (the
  same store the WASM client wraps) as a `MailListMirror`: seeded like the web
  adapter's `seedOpenedView`, folding `message.updated` like
  `storeUpdatesFromEvent`, re-projecting rows like `projectView`. Un-stales
  the L3 convergence test by fulfilling the contract's client half. Interim
  wart, deleted in Slice 2: the mirror re-derives the predicate by parsing the
  `in:<account>/<mailbox>` query string — exactly the duplication D177 exists
  to kill.

- **Slice 1 — contract + runtime decision (vocabulary v1).**
  `membership_vocabulary` on `RuntimeCallerCapabilities`; `ViewMembership` on
  the open-view result + `ViewSnapshot`; the runtime resolves v1 predicates
  (concrete `in:` scope; role smart mailboxes; All Mail) from its own mailbox
  tables. `view_registry` consumes the runtime-computed membership — the
  descriptor flag is still accepted from legacy clients (stamp → treated as a
  v1 capability declaration) but no longer read by the event pump directly.
  L2 tests: assignment matrix (capability × view shape → membership),
  including the D178 fallbacks.

- **Slice 2 — client + testkit adoption.** Web: declare the capability,
  feed the response predicate into the entity store, delete
  `resolveMailListPredicate`/`buildMailListPredicateContext`/`isMailListSelfMaintained`
  and the descriptor stamp. Testkit: `MailListMirror` reads the contract off
  the open result (query-string parse deleted); `ViewWatch` self-maintains
  exactly when told to. Both generated schemas regenerated (web + mcp — the
  nightly.7 lesson).

- **Slice 3 — retire the flag.** Remove `client_self_maintained` from
  `ViewDescriptor` (contract-core), the wire serde shim, the bench/testkit
  stamps, and the legacy acceptance path from Slice 1. One grep proves it
  gone.

- **Slice 4 — compiler-proven predicates (vocabulary v2, D179).** The
  projection-IR backend beside the SQL backend on the same `MailQueryRule`
  AST; the projector's `ViewPredicate` grows the IR arm; user smart mailboxes
  with provable rules become `ClientMaintained`. Differential L2 property
  test: for arbitrary rules and message projections, SQL backend and IR
  backend agree on membership (the same one-engine-two-backends discipline as
  the NS1 `_effective` cutover).

- **Slice 5 — graduate the L3 gate.** With the convergence test riding the
  real contract and the suite proven reliable, move `stalwart-integration`
  from `continue-on-error` (soft) to a hard CI gate — the original graduation
  condition on the job.

## 5. Test contract

- Assignment matrix at L2 (Slice 1): every (capability, view shape) cell.
- Differential predicate property test at L2 (Slice 4): SQL ≡ IR.
- The L3 convergence test (Slice 0/2) is the end-to-end staleness canary:
  real Stalwart → sync → firehose → client store → rows.
- A deliberate-mismatch L2 test pinning D178: a caller declaring vocabulary
  v1 opening a v2-only view MUST get `RuntimeServed` frames.

## 6. Links

- docs/issues/L2-single-source-view-membership — option iii's ledger; step 4
  ("negotiate the mode") is superseded by D176/D177.
- RFC-L2-client-resilience — parent front (M49/M50 reactive-store completion).
- RFC-L2-client-replication-model §6 — the one-fold/two-backends north star
  D179 instantiates for predicates.
- DESIGN-L2-test-taxonomy — the convergence cell + the graduated L3 gate.
