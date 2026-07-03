# Feasibility: Posthaste → multi-domain platform (Calendar)

Owner question (2026-07-03): how hard to build a Calendar app on Posthaste's
structure, integrate mail+calendar, and write cross-domain agent automations
(`tag:schedule → agent → calendar MCP → add event`)? Evidence-based survey
(read-only, opus). Verdict + the parts that matter below; full crate table in
the survey transcript.

## The honest headline
The "~60-70% of the stack is domain-agnostic" claim is **true of the
distributed-systems *difficulty*** (the kernel, link engines, tap, macaroon
crypto — the hardest-to-build spine) but **only ~30-40% of the *lines***. A
calendar still writes a full domain layer (gateway, schema, service, UI) and
eats ~3-4 engineer-months of irreducible calendar-specific work that no
substrate lever touches. Both facts are true at once.

## Where mail enters the generic machinery — five narrow, named seams
1. **Convergence impl** — `MessageConvergence` (replica-core `convergence.rs:537`)
   is the sole mail impl of the generic `OptimisticReplica<C: Convergence>`.
   Calendar adds `EventConvergence`; `Replica<C>` is reused unchanged. The doc
   already anticipates this ("a future fold adds another impl").
2. **Operation vocabulary** — `MailOperation` (one enum, flattened into
   `MutationRequest`, contract-core `lib.rs:724`). Calendar writes a parallel
   `CalendarOperation`; there is no generic carrier yet.
3. **Projector row types** — `replica-projector` is the ONE substrate crate NOT
   parameterized over `C` (hardcodes `MessageReplica`, mail fields). Generalizing
   it is the forcing function a second domain creates.
4. **Query field vocabulary** — `SmartMailboxField` + a prefix match; `before:`/
   `after:` + date operators already exist. Adding `attendee:`/`calendar:` is a
   3-site edit.
5. **Store schema + authz vocabulary** — mail DDL vs event DDL (the pool/repair/
   txn/staging engine is reused as-is); add a `schedule` verb + `calendar` axis
   (2 enums + route-table entries).

## The genuinely-hard calendar work (no substrate help) — ~3-4 eng-months
- **RRULE recurrence** (RFC 5545), 3-6wk — interacts badly with the optimistic
  fold (editing one occurrence is an effect over a *virtual* entity).
- **Timezone correctness** (VTIMEZONE/DST/floating), 2-4wk — the pinned `time`
  crate isn't a full tz DB; add chrono-tz/jiff.
- **iTIP/iMIP invitations** (RFC 5546/6047), 3-5wk — **the natural mail↔calendar
  seam: invites ARRIVE as `text/calendar` MIME in email.** Zero iCalendar
  handling exists today. This is the enabling primitive for the owner's exact
  automation and the one place the two domains genuinely couple.
- **Free/busy** (CalDAV scheduling), 2-3wk read-only first.

## Effort, calibrated
- **(a) The integration alone** (mail rule → agent → calendar MCP): works TODAY
  with ~zero framework change — the `tag:… → attenuated-token webhook/exec →
  agent → MCP` path already ships. New pieces: a calendar MCP server (a
  CalDAV/Google shim in the existing `apps/mcp` TS harness) + optionally a
  calendar-scoped token verb. **~1-2 weeks; a demo in days.**
- **(b) Calendar app on the substrate**: free = kernel, both link engines,
  call-policy, provider-call, macaroon mint, tap/FactLog, store DB-infra, config/
  secret/push ports, MCP generation. Fresh = EventConvergence+fold,
  CalendarOperation, a forked/generalized projector, a CalDAV/Google gateway
  (~5-10k lines), event schema+row-mapping, CalendarService, calendar UI.
  **~4-6 eng-months** (of which ~2mo is the substrate-free §hard work).
- **(c) Framework extraction itself**: RFC-scale, ~2-4 eng-months. Three hardest
  steps: generalize Convergence *into the projector*; parameterize the operation
  vocabulary while preserving the one-vocabulary invariant; the app-shell + node
  composition roots (largest in lines). Only pays off with a committed 2nd domain.

## Recommended path (de-risked, value-first)
**Do NOT start with the framework extraction.** Start with the **integration
shim** (path a): stand up a throwaway calendar MCP (CalDAV/Google) exposing
`add_event`, wire the existing `tag:schedule → webhook/exec → agent → calendar
MCP` rule. Days-to-two-weeks, zero substrate change, immediately proves the
actual user value and surfaces the one concrete gap (calendar-scoped token
verbs). THEN, only if it lands and a second domain is genuinely committed, do the
extraction as a governed RFC with **the calendar as the forcing second
instantiation** — let it prove each generalization rather than generalizing on
speculation. This turns the biggest risk (a multi-month extraction that might not
pay off) into value-first, de-risked steps.
