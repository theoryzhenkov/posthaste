# RFC-L2: Typed, single-source query schema (rules + live queries)

> Refactor the shared mail-query model so relative dates roll correctly, the
> field/operator/value validity has one source of truth, and invalid conditions
> are rejected at the boundary. Forcing function: the relative-date freeze bug.
> Grounded in the 2026-07-06 investigation.

## Context — what exists today

Rules and smart mailboxes (live queries) **already share one query system**; the
perception that they're separate is naming + two editor entry points, not
architecture:

- **One AST** — `SmartMailboxRule → SmartMailboxGroup → SmartMailboxCondition`,
  with `SmartMailboxField` / `SmartMailboxOperator` / `SmartMailboxValue`
  (`crates/posthaste-domain-model/src/model/smart_mailboxes.rs:43,77,93,105,121`).
- **Three front-ends → the same AST**: the visual editor (web
  `RuleGroupEditor`/`ConditionEditor` + A/R1's `FIELD_REGISTRY`), the rules editor
  (`AutomationRuleEditor`, same shared components), and a **text grammar**
  (`posthaste_query_grammar::parse_query_with_scopes`, used by
  `crates/posthaste-authority-server/src/mail_queries/rules.rs:3,8`).
- **One evaluator** — the store SQL compiler (`field_compilers.rs`
  `compile_simple_field:34`/`compile_text_field:59`/`compile_date_field:90`,
  dispatched by `rule_compiler.rs:73`), driven by
  `posthaste-domain-service/src/service/smart_mailbox_queries.rs:57,110`.

### The problems (all in the *value* layer + its duplicated validity)

1. **Stringly-typed value** — `SmartMailboxValue { String | Strings | Bool }`
   (`smart_mailboxes.rs:93`, `#[serde(untagged)]` at `:91`). No date/relative
   type, so dates are strings.
2. **Relative-date freeze bug** — the web `DateValueWidget` resolves "Within N"
   with `relativeDateValue(amount, unit, new Date())` **at edit time**
   (`conditionValueWidgets.tsx:197`) and stores an absolute timestamp; the
   compiler (`compile_date_field`) does a plain comparison, no `now`. So a rule
   "received within 7 days" freezes to a fixed date and stops rolling. It also
   *reads* as nonsense ("before within 7 days").
3. **Duplicated field→type→operator validity** — encoded in Rust (the
   `field_compilers` dispatch + each type-compiler's operator `match`) **and** in
   the web `FIELD_REGISTRY` (A/R1). They can drift → the editor offers an operator
   the compiler rejects → a runtime `StoreError::Failure` deep in SQL.
4. **Date-centric operator names reused for numbers** — `Before/After/OnOrBefore/
   OnOrAfter` (`smart_mailboxes.rs:77`) are also used for numeric `Size` (`< > <= >=`).
5. **Untagged value enum** — fragile once date/relative variants are added.
6. **Naming** — everything is `SmartMailbox*` though rules use it too.

## Decisions

### D1 — Typed, tagged value model
Replace `SmartMailboxValue` with a **tagged** enum (mirroring the group node's
`#[serde(tag = "type")]` at `:132`) carrying explicit variants:
```rust
#[serde(tag = "type", rename_all = "camelCase")]
pub enum MailQueryValue {
    Text(String),           // was String
    TextList(Vec<String>),  // was Strings (In)
    Bool(bool),
    Number(i64),            // Size etc. — numeric, not stringly
    Date(DateValue),
}
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DateValue {
    Absolute(String),                 // RFC3339 instant, compared as stored
    Relative { amount: u32, unit: DateUnit }, // "-7 days", stored as-is, rolls
}
pub enum DateUnit { Minutes, Hours, Days, Weeks, Months }
```
**Wire migration** (a format 3 front-ends + stored settings/config share): the
value deserializer accepts BOTH the legacy untagged shapes (`"x"`, `["x"]`,
`true`) and the new tagged shapes during a transition window, and a one-time
migration rewrites stored smart mailboxes / rules in `AppSettings` + config TOML.
Gate on a schema version (`PRAGMA user_version` / a settings schema tag).

### D2 — Relatives evaluated at query time (rolls; the bug fix)
`compile_date_field` emits `datetime(m.received_at) < datetime('now', '-7 days')`
for a `Relative` (unit → SQLite modifier); `Absolute` compares as stored. Because
the model + compiler are shared, this fixes relative dates in smart mailboxes,
rules, AND the text grammar at once. The store test asserts a `Relative` condition
matches on a *rolling* window (evaluate at two `now`s).

### D3 — Natural relative UX
With a real `Relative` type the editor offers dedicated readings — **"in the last
N"** / **"more than N ago"** — instead of "before/after + Within". A/R1's
`FIELD_REGISTRY` already knows each field's type, so it emits the right
`MailQueryValue` variant; the confusing free "Within" mode is removed.

### D4 — One canonical field schema (kills the Rust↔TS drift)
Define the schema ONCE in Rust: `field → { value_type, allowed_operators }`
(a table/`const`, e.g. `MailQueryFieldSpec`). The compiler dispatch is **driven
from it** (no second operator `match` per type that can disagree), and it is
**exported to the web via the openapi/codegen boundary** so the web
`FIELD_REGISTRY` is *generated*, not hand-maintained. Editor, compiler, and
grammar can no longer disagree.

### D5 — Boundary validation (invalid = rejected at the edge)
Validate a condition against the D4 schema at **deserialize/construct** time (a
`validate()` or a smart constructor) — a `Contains` on a boolean, or a `Text`
where a `Date` belongs, is rejected at the API/editor boundary with a clear
message, not as a deep `StoreError::Failure`. Turns runtime SQL errors into
boundary errors.

### D6 — Neutral operator names
Rename operators to neutral comparisons (`Eq, In, Contains, Lt, Gt, Le, Ge`); the
type-directed editor **labels them per field type** ("before/after" for dates,
"smaller/larger than" for size). The model stops speaking "date". (Serde wire
names change → covered by the D1 migration window.)

### D7 — Rename `SmartMailbox*` → `MailQuery*` (no-logic)
`MailQueryField/Operator/Value/Condition/Group/Rule/RuleNode`. Pure type rename;
serde field names (the wire) are unchanged, so it is wire-safe. Its own commit,
no behavior change — makes "one query system, several front-ends" obvious.

### D8 — Confirm one evaluator
Verify a rule matches an incoming message by reusing the one SQL compiler (scoped
to the new message id) rather than a separate matcher; if a divergent per-message
path exists, consolidate it onto the shared compiler. (Investigation confirmed the
shared model; the eval reuse is to be confirmed in R5b.)

## Slices (relative-date fix ships first)

- **R5a — the bug fix (user-visible, ship first):** D1 (typed/tagged value +
  legacy-compat deserialize + stored-query migration) + D2 (rolling relative eval)
  + D3 (natural relative UX). This alone fixes the owner-reported bug.
- **R5b — single-source schema:** D4 (canonical Rust schema drives the compiler +
  generates the web registry) + D8 (confirm one evaluator).
- **R5c — validation + neutrality:** D5 (boundary validation) + D6 (operator
  rename/relabel).
- **R5d — the rename:** D7 (`SmartMailbox*` → `MailQuery*`), mechanical, last so it
  doesn't churn the earlier diffs.

## Risks
- **Wire-format migration** is the main risk — 3 front-ends + stored
  settings/config share `SmartMailboxValue`. Mitigate: dual-read (legacy untagged
  + new tagged) during the window + a versioned one-time migration + a round-trip
  test over real stored queries. Do NOT drop legacy read until a migration has run
  on a nightly.
- SQLite date modifiers vs stored `received_at` format (RFC3339 vs epoch) —
  `compile_date_field` must use a consistent `datetime()` wrapping on both sides.
- The text grammar (`posthaste_query_grammar`) must learn the relative syntax or
  explicitly reject it (no silent absolute freeze there either).

## Out of scope
Cc/Bcc columns (needs storage change); new fields/operators beyond the value-model
work; the action-registry (separate track); nesting (already exists).
</content>
