# DESIGN-L2-rule-actions — the release rules/actions system

Status: implemented (this document describes the shipped design).
Scope: the event-bus automation rules (RFC-L2-scripting ruling 23) — the action
vocabulary, its registry across layers, the condition editor's value-widget
system, and the destroy/exec safety posture.

## 1. Problem

Before this slice the actions system was a working skeleton with three classes
of incompleteness:

1. **Vocabulary.** Writable actions were tag / move / notify / emit / webhook.
   No mark-read/unread, no flag, no archive/junk/trash filing, no permanent
   delete — a paying user could not express the bread-and-butter MailMate-class
   rules ("junk this sender", "archive receipts", "delete this newsletter
   permanently").
2. **Per-cell wiring in the editor.** Capabilities (address autocomplete, the
   mailbox/account/role pickers) were bolted onto exactly one
   (valueType × operator-arity) cell. The dispatch special-cased the `in`
   operator into a bare comma-separated text box BEFORE the value-type switch,
   so every capability vanished the moment the user chose "is one of" — the
   owner-reported bug verbatim.
3. **A silently broken executor.** The engine applied `Tag` through
   `MailOperation::SetUserTags`, but the direct-apply bridge
   (`MailCommandRequest::from_operation`) rejected that op as "replica-only":
   every Level-0 tag rule failed at runtime with a warning nobody saw. No test
   covered a Level-0 action end to end.

## 2. The action registry (one definition per layer, compiler-enforced)

There is no single physical "registry file" spanning Rust and TS; instead each
layer has exactly ONE authoritative table for the vocabulary, and every table
is **exhaustively typed against the previous layer**, so adding action N+1 is
one forced edit per layer — the compiler walks you through all of them:

| layer | the one definition | drift protection |
|---|---|---|
| domain model | `RuleAction` + `WritableRuleAction` enums, `kind_str`, `is_destructive`, `validate_rule_action` (`crates/posthaste-domain-model/src/model/rules.rs`) | exhaustive `match`es; `From<WritableRuleAction>` lift |
| config (TOML) | none — `rules.toml` / `rules.d` deserialize the domain enum **verbatim** | serde is the mapping; round-trip tests in `rules/writer.rs` |
| REST boundary | `WritableRuleInput.action: WritableRuleAction` (`posthaste-server/src/rules_api.rs`) | serde; `utoipa` derives the OpenAPI schema from the same enum |
| generated clients | `openapi.json` → `apps/web/src/api/schema.gen.ts`, `apps/mcp/src/schema.gen.ts` | `openapi_contract` test + `api:check` drift gates |
| web presentation | `ACTION_REGISTRY: Record<ActionKind, ActionDescriptor>` (`apps/web/.../automations/ruleActionHelpers.ts`) — label, hint, default, summary, `destructive` | `Record` over the TS kind union: a missing/new kind fails `tsc` |
| web forms | one branch per kind in `RuleActionEditor.tsx` | the `WritableRuleAction` union narrows per branch |
| executor | `level0_operation` — the ONE pure action→`MailOperation` projection (`posthaste-authority-server/src/rules/engine.rs`), plus `action_precondition` for idempotence | exhaustive `match`; unit-tested per kind |

Two deliberate non-goals: (a) no runtime-generated forms (a webhook form with
grant checkboxes and security callouts is not expressible as a generic
param-list renderer without building a worse form system), and (b) no second
schema artifact for actions — the OpenAPI contract already carries the wire
truth and both clients regenerate from it.

### 2.1 The shipped vocabulary

Writable (GUI/REST/`rules.d`):

| kind | params | executor projection | precondition (idempotence/loop-break) |
|---|---|---|---|
| `tag` | `tag` | `SetUserTags` → SetKeywords command | NOT keyword = tag |
| `move` | `mailboxId` | `ReplaceMailboxes` | NOT mailboxId = target |
| `moveToRole` **(new)** | `role` ∈ archive/junk/trash/inbox | role → mailbox id resolved at the engine (same lookup as `move_message_to_role`), then `ReplaceMailboxes`; unmapped role ⇒ failed outcome + warning | NOT mailboxRole = role |
| `markRead` **(new)** | `read: bool` | `SetReadState` → `$seen` toggle | NOT isRead = read |
| `flag` **(new)** | `flagged: bool` | `SetFlaggedState` → `$flagged` toggle | NOT isFlagged = flagged |
| `notify` | `title`, `body?` | dedicated (fact + log, no operation) | none |
| `destroy` **(new)** | — | `MailOperation::Destroy` — the EXISTING delete-permanently machinery | none (a destroyed row cannot re-match) |
| `emit` | — | none (the `rule.fired` fact is the output) | none |
| `webhook` | `url`, `grants`, `expirySeconds` | hook path (scoped token, bounded retry, dead-letter) | none (dedupe via idempotency key) |

Config-file-only: `exec` (unchanged; see §4).

**Deliberately left out**, and why:

* **snooze** — needs a return-time parameter and the paired unsnooze
  lifecycle; a rule that silently hides mail on a broad match is a footgun
  disproportionate to its release value. `moveToRole` explicitly refuses the
  `snooze` role for the same reason (a bare role-move there would hide mail
  with no return time).
* **forward / redirect** — requires driving the compose/send pipeline from the
  engine plus mail-loop protection (X-Loop headers, hop counts). Post-release;
  the webhook action already covers the automation-integration need.
* **setKeyword as a distinct kind** — user tags ARE keywords in this model
  (`SetUserTags` folds to the SetKeywords command); `tag` covers it.
* **stop-processing as an ACTION** — shipped instead as a **rule field**
  (`stopProcessing`), because it modifies evaluation flow, not the message;
  an action-shaped "stop" cannot compose with a real action on one rule.

### 2.2 Chaining semantics (now explicit)

Previously implicit; now documented on `Rule` and pinned by e2e:

* For one triggering fact, every enabled rule whose topics + WHEN-clause match
  runs, **in order**: authored `rules.toml` rules first (file order — the
  operator controls it), then GUI-managed rules sorted by case-insensitive
  name, then id (`load_rules` sorts; `read_dir` order is platform-arbitrary,
  which would make chaining non-deterministic).
* A matched rule with `stopProcessing = true` ends the walk for that fact.
* Caveat (inherent to a fact-bus engine, documented rather than hidden): a
  Level-0 action's own `message.updated` is a NEW fact and re-enters
  evaluation; preconditions make the acting rule a no-op on re-entry, but a
  later rule can match the follow-up fact. `stopProcessing` is per-fact, not
  per-message-lifetime.

## 3. The value-widget system (capability × arity, not per-cell)

`conditionValueWidgets.tsx` now has ONE registry:

```
VALUE_WIDGETS: Record<ConditionValueType, { Scalar, ListEntry?, listPlaceholder? }>
```

composed with the operator's arity at dispatch: scalar operators render
`Scalar`; the `in` operator renders the generic `ListValueEditor` (removable
chips + the type's `ListEntry` adder, emitting the same `string[]` wire shape).
Capabilities hang off the VALUE TYPE, so they survive every operator the
generated field schema (`querySchema.gen.ts`, still the single source of which
field × operator combinations exist) admits:

* `address` → the compose-shared address-book autocomplete
  (`RecipientSuggestionInput`, extended with `onPick`/`onEnter` for list-entry
  commits) in BOTH arities — the fix for "switch to 'is one of' and
  autocomplete stops working".
* `keyword` → live tag suggestions (new `TextSuggestionInput` + `Tag/list`
  read), both arities — keyword conditions previously had no autocomplete at
  all.
* `mailboxRef` / `accountRef` / `roleEnum` → the existing pickers, now also as
  `in`-list adders (each pick appends an entry).
* `boolean` / `date` / `size` → scalar-only (their schema rows never offer
  `in`); the plain-text entry is the fallback safety net.

Suggestion sources live in `rule-group/suggestionSources.ts` as hooks — the
data half of a value type's capability, shared by both arities by
construction.

### 3.1 Autocomplete root causes fixed

1. `in`-operator dispatch preceded the type switch
   (`conditionValueWidgets.tsx`, old line 80) — all capabilities lost under
   "is one of". Fixed by the registry composition above.
2. Autocomplete was a property of three fields' scalar widget, not of value
   types — keyword fields (and the tag-action input's cousin, the keyword
   condition) had none, reading as "autocomplete often doesn't work". Fixed by
   the per-type suggestion sources.
3. Staleness in the settings SURFACE window: standalone surfaces run WITHOUT
   the live event bridge (`App.tsx` gates `DaemonEventBridge` off), and the
   address book is populated by a deferred post-startup backfill
   (`backfill_address_book`) — a `senderAddresses` query cached before the
   backfill (or before new mail) pinned an empty/stale book; the only
   event-invalidation was compose-send (`invalidations.ts`). Fixed with
   `refetchOnMount: 'always'` on both suggestion queries — one small REST call
   per editor open buys correctness in every window.
4. Comma-tokenized filtering in single-value inputs:
   `filterAddressSuggestions` always filtered on the text after the last
   comma; a scalar value containing a comma (`"Doe, John"`) silently filtered
   on the fragment. The filter now takes a `'token' | 'whole'` mode;
   `RecipientSuggestionInput` derives it from `selectionMode` (replace →
   whole). The match itself was and remains case-insensitive SUBSTRING over
   name/email/label — now pinned by tests so it cannot regress to
   prefix-matching.

## 4. Destroy and the exec boundary (safety posture)

**Destroy** is conservative by construction:

* distinct explicit wire kind `destroy` — never overloaded onto move/trash;
  `moveToRole: trash` is the recoverable neighbour and is labelled
  "(recoverable)" in the editor to sharpen the contrast;
* executes through the EXISTING `message.destroy` operation — no new deletion
  primitive, and it only ever runs on the message the boundary-validated
  WHEN-clause query just matched (the engine re-runs the scoped query per
  fact; there is no path around the stored, validated rule);
* `validate_rule_action` refuses a destroy whose WHEN-clause has no leaf
  condition, at EVERY ingress: the REST write path (400), the managed
  `rules.d` loader (skip + warn), the authored `rules.toml` loader (load
  error), and the editor mirrors the same guard client-side (save disabled +
  explanation) so the user never round-trips a 400;
* the editor renders it unmistakably: destructive styling in the picker, a red
  hint, an irreversibility callout, and a red summary line in the rule list.

**Exec** remains config-file-only and STRUCTURAL: it is not a variant of
`WritableRuleAction` (a `{"kind":"exec"}` body fails serde with a 422 before
any handler), not a row in the web `ACTION_REGISTRY` (whose key type cannot
name it), skipped by the `rules.d` loader, and refused by the writer as
defence in depth. Tests pin every layer: the domain serde-boundary test, the
`rules_crud_e2e` REST rejection, the loader-skip test, and the web registry
parity test.

Shared validation (`validate_rule_action`, domain model) also carries the F1
empty-grant-hook rule and the `moveToRole` role allowlist, so no store can
smuggle a rule another store would refuse.

## 5. Executor bridge fix

`MailCommandRequest::from_operation` (posthaste-authority-server-link) now
projects the keyword-shaped semantic ops — `SetUserTags`, `SetReadState`,
`SetFlaggedState` — onto the SetKeywords direct-apply command (the same folding
the far node's `apply_operation` performs), instead of rejecting them as
replica-only. This un-breaks the pre-existing Tag action and is what
`markRead`/`flag` ride. Role moves stay replica-only on the bridge (they need
the account's role→mailbox map); the rule engine resolves the role itself via
`MailService::list_mailboxes` and applies a `ReplaceMailboxes`.

## 6. Test map

* domain model — writable-kind round trips, exec unrepresentable, destroy/when
  guard, role allowlist, `stopProcessing` wire default.
* authority-server-link — keyword-shaped op projections; role-move rejection.
* engine — `level0_operation` per kind (incl. destroy targeting + unmapped
  role failure), preconditions per kind, rule ordering, delete-eviction.
* writer/config — per-kind TOML round trips, unconditional-destroy refusal at
  write AND load, `stopProcessing` round trip, deterministic merge order.
* `rules_actions_e2e` (bundled server) — REST boundary 400s, per-kind 201s,
  `stopProcessing` chaining (later rule provably never fires), live
  tag/markRead/flag executor dispatch, and destroy's destructive path
  (exactly the matched message vanishes; the `rule.fired` destroy fact lands).
* web — registry parity/destructive/exec-exclusion, list-editor wire-shape
  helpers, filter-mode + substring/case pins, and widget-composition tests
  (address×in autocomplete, mailbox×in picker, keyword suggestions).
