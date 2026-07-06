---
scope: L2
summary: "REVIEW — Posthaste's action/query system (mail rules + smart-mailbox queries) benchmarked against MailMate. Maps the current query model, operator/field/action coverage, and editor UX; a gap table vs MailMate; and prioritized, opinionated recommendations centred on a type-directed condition editor over the existing model. Read-only analysis; no code changes."
modified: 2026-07-06
reviewed: 2026-07-06
lifecycle: ephemeral
type: REVIEW
state: draft
depends:
  - path: eph/RFC-L2-scripting
dependents: []
---

# REVIEW — Action / Query System vs MailMate

**Owner's ask:** *"MailMate allows much more, is smarter, auto-filling the correct value format on creation."* This review grounds that instinct in code and says **exactly** what to add.

**Headline finding.** The Rust model is in good shape — it already has recursive AND/OR/NOT groups, a shared grammar, and a real SQL evaluator. The gap the owner feels is almost entirely in **the web condition editor**: the value input is a dumb text box for every field except booleans. The single highest-value change is a **type-directed condition editor** (a field→type→operators→value-widget registry) — an *editor-only* win over the model that already exists. The model-side gaps (a few missing fields/operators) are real but secondary and can follow.

---

## 1. What we have

### 1.1 The query model (one grammar, one tree)

There is **one** query representation, shared by smart mailboxes AND the rules engine's WHEN-clause (RFC-L2-scripting ruling 4). It is the `SmartMailboxRule` tree, defined in `crates/posthaste-domain-model/src/model/smart_mailboxes.rs`:

- **Recursive boolean tree — already present.** `SmartMailboxRuleNode` (`smart_mailboxes.rs:126`) is `Group | Condition`; `SmartMailboxGroup` (`smart_mailboxes.rs:97`) has `operator: All|Any` (`smart_mailboxes.rs:32`), a `negated: bool`, and `nodes: Vec<SmartMailboxRuleNode>` — so **nested AND/OR groups with NOT at both group and condition level exist in the model today.** This is a critical, often-missed fact: the schema is NOT a flat list.
- **Leaf condition:** `SmartMailboxCondition { field, operator, negated, value }` (`smart_mailboxes.rs:113`).
- **Fields** — `SmartMailboxField` (`smart_mailboxes.rs:43`), 17 variants: `SourceId, SourceName, MessageId, ThreadId, ConversationId, MailboxId, MailboxName, MailboxRole, IsRead, IsFlagged, HasAttachment, Keyword, FromName, FromEmail, Subject, Preview, ReceivedAt`.
- **Operators** — `SmartMailboxOperator` (`smart_mailboxes.rs:69`), 7 variants: `Equals, In, Contains, Before, After, OnOrBefore, OnOrAfter`.
- **Value** — `SmartMailboxValue` (`smart_mailboxes.rs:85`), untagged `String | Strings(Vec<String>) | Bool`.

### 1.2 The evaluator (SQL compiler)

`crates/posthaste-store/src/smart_mailboxes/rule_compiler.rs` compiles the tree to parameterized SQL:
- `compile_smart_mailbox_group` (`rule_compiler.rs:18`) is recursive, joins nodes with `AND`/`OR`, wraps in `NOT (...)` when negated — full boolean nesting is honoured end to end.
- `compile_smart_mailbox_condition` (`rule_compiler.rs:53`) dispatches per field to `field_compilers.rs`. Operator support is **type-gated in the compiler**: text fields accept `Equals/Contains/In` (`field_compilers.rs:59`), simple/id fields `Equals/In` (`field_compilers.rs:34`), date fields the four inequality ops (`field_compilers.rs:90`), bool fields only `Equals` (`field_compilers.rs:114`). An operator the field's compiler doesn't handle returns a `StoreError` at evaluation time (e.g. `field_compilers.rs:50,82,103,118`). **This per-type operator matrix is the de-facto type system the editor should mirror.**
- The message table (`crates/posthaste-store/src/db/schema/sql.rs:20`) has columns the model does **not** yet expose: `to_json` (recipients), `size`, `rfc_message_id`, `in_reply_to`. There is an FTS5 index (`sql.rs:326`) over `subject/from_name/from_email/preview` (body is not indexed).

### 1.3 The text grammar (search bar → same tree)

`crates/posthaste-query-grammar/src/` parses `prefix:value` search strings into the **same** `SmartMailboxRule`:
- `nodes.rs:12` `parse_prefixed` maps prefixes: `from/f/sender`, `subject/s`, `body/preview`, `is`, `has`, `tag/keyword`, `in/mailbox`, `source/account`, `id`, `thread`, `conversation`, `before`, `after`, `date`, `newer`, `older`.
- `from:` expands to an `Any(FromEmail contains, FromName contains)` group (`nodes.rs:118`); free text expands across From/Subject/Preview (`nodes.rs:242`) — the grammar synthesizes small OR-groups, exercising the recursive model.
- Relative dates: `newer:7d`/`older:2w` computed against now (`date.rs:46`), absolute `date:YYYY-MM-DD` becomes a same-day range (`date.rs:4`). **Notable:** the grammar has **no top-level `OR` / parenthesis syntax** — tokens are `AND`-joined at the root (`lib.rs:87`). OR only appears inside the hand-built expansions.

### 1.4 The action / rules layer — TWO systems

There are two distinct rule systems sharing the WHEN tree:

**System A — event-bus `Rule` (`/v1/rules`, RFC-L2-scripting S5).** `crates/posthaste-domain-model/src/model/rules.rs:35`. `when: SmartMailboxRule`, `on: Vec<String>` trigger topics, one `action: RuleAction`. Actions (`rules.rs:76`): `Tag, Move, Notify, Emit, Webhook, Exec`. Evaluated at the authority server on the tap; `match_message` (`crates/posthaste-authority-server/src/rules/engine.rs:353`) runs the WHEN tree as a single-row query. Stored in a config root: hand-authored `rules.toml` (may contain exec) merged with GUI-managed `rules.d/*.toml` (`crates/posthaste-authority-server/src/rules/config.rs:97`).

**System B — ingestion-time `AutomationRule` (AppSettings).** `crates/posthaste-domain-model/src/model/automation.rs:10`. `condition: SmartMailboxRule`, `triggers: Vec<AutomationTrigger>` (MessageArrived/Changed/Manual), **multiple** `actions: Vec<AutomationAction>`, plus `backfill`. Actions (`automation.rs:105`): `ApplyTag, RemoveTag, MarkRead, MarkUnread, Flag, Unflag, MoveToMailbox`. Normalized in `crates/posthaste-authority-server/src/mutations/automation.rs:23`; preview via `POST /v1/automation-rules:preview` (`mutations/automation.rs:4`).

**Security (HARD CONSTRAINT — confirmed intact).** `RuleAction::Exec` is config-file-only. The REST write body deserializes into `WritableRuleAction` (`rules.rs:156`), which has **no `Exec` variant**, so `{"kind":"exec"}` is unrepresentable at the serde boundary (`rules.rs:333` test). The managed loader also defensively skips any exec found in `rules.d` (`config.rs:163`). **Any action recommendation below preserves this: exec must never become GUI/REST-settable — it is RCE.**

### 1.5 The editor UX (apps/web)

All three surfaces (System A rules, System B automations, smart mailboxes) funnel their WHEN/condition tree through **one shared recursive editor**:
- `apps/web/src/components/settings-panel/RuleGroupEditor.tsx:34` — recursive; per-group `all`/`any` select and `not` checkbox (`RuleGroupEditor.tsx:53`), "+e" adds a condition, "+g" adds a nested group (`RuleGroupEditor.tsx:90,105`), nested groups recurse (`RuleGroupEditor.tsx:162`). **The full boolean tree IS exposed in the UI.**
- `apps/web/src/components/settings-panel/rule-group/ConditionEditor.tsx:24` — one condition row.
- Option tables: `apps/web/src/components/settings-panel/helpers/smartMailboxForms.ts`.

**Operators already adapt to the field.** `operatorOptionsForField` (`smartMailboxForms.ts:107`) returns the correct operator subset per field, mirroring the Rust compiler's matrix (id→`equals/in`, text→`equals/contains/in`, bool→`equals`, date→four inequalities). Changing the field resets to the field's first valid operator (`ConditionEditor.tsx:99`).

**The value input does NOT adapt — this is the owner's "not smart" pain, exactly.** `ValueEditor` (`ConditionEditor.tsx:168`) branches on only two things:
1. boolean field → a `true/false` `<Select>` (`ConditionEditor.tsx:181`);
2. everything else → a **single generic `<Input>` text box** (`ConditionEditor.tsx:199`), with the `in` operator merely comma-splitting the same text box into an array.

So `receivedAt before` gives a bare text box (user must hand-type an ISO timestamp), `mailboxId equals` a bare text box (hand-type a raw mailbox id), `fromEmail` a bare text box (no address autocomplete), `keyword`/`mailboxRole` bare text boxes (no known-value dropdown). New conditions default to `mailboxRole equals ""` (`smartMailboxForms.ts:134`) — an id field with a free-text value and no picker.

**Inconsistency worth noting:** System B's `moveToMailbox` action DOES use a real mailbox-picker `<Select>` backed by a live mailbox query (`automation-actions/ActionListEditor.tsx:192`), while System A's `move` action uses a **plain text "Mailbox id" field** (`automations/RuleActionEditor.tsx:120`). The building block for type-aware inputs already exists in the codebase — it is simply not used in the condition editor.

**Dead capability:** `conversationId` exists in the model and generated schema (`schema.gen.ts:2653`) but is omitted from the UI `FIELD_OPTIONS` (`smartMailboxForms.ts:48`) — unreachable in the builder.

**The search bar is a separate code path** (`apps/web/src/queryDefinitions.ts`, `query-language/parser.ts`) with its own prefix tokens; it shares no components or option tables with the condition builder. It parses to the query grammar server-side, not via shared client code.

---

## 2. Gap analysis vs MailMate

MailMate's rule/smart-mailbox editor is the bar: rich field coverage, a wide per-type operator set, nested boolean groups, and — the owner's key point — a **value input whose widget is inferred from the chosen field+operator** (date picker for dates, address autocomplete for addresses, a header-name field for arbitrary headers, number+unit for size, an address-book toggle, etc.).

| Capability | Posthaste today | vs MailMate | Impact |
|---|---|---|---|
| **Nested AND/OR groups** | **Have** — model (`smart_mailboxes.rs:97`), evaluator (`rule_compiler.rs:18`), editor (`RuleGroupEditor.tsx:162`) | Parity | — (a genuine strength; do not rebuild) |
| **NOT / negation** | **Have** — group- and condition-level `negated` | Parity | — |
| **Type-directed VALUE input** | **Missing** — generic text box for all but booleans (`ConditionEditor.tsx:199`) | MailMate infers widget from field+operator | **HIGH — the owner's core complaint.** Forces users to hand-type ISO dates, raw ids, exact keywords |
| **Per-field operator subsetting** | **Have** — `operatorOptionsForField` (`smartMailboxForms.ts:107`) | Parity | — (already "smart" here) |
| **Date: relative + absolute** | Partial — grammar has `newer/older/date` (`date.rs`); **editor** has none (bare text for `before/after`) | MailMate has a relative+absolute date picker | **HIGH** — the marquee "auto-fill correct format" case |
| **Address fields (From)** | Partial — `FromEmail/FromName`; no `To/Cc/Bcc`; no autocomplete | MailMate: To/Cc/Bcc/any-recipient + address-book + autocomplete | **MED-HIGH** — filtering by recipient is table-stakes |
| **To/Cc/Bcc / recipient fields** | **Missing** as model fields (data present in `to_json`, `sql.rs`) | MailMate: full recipient set | **MED-HIGH** |
| **Arbitrary header match** | **Missing** | MailMate: any header by name | LOW-MED (needs raw headers; not stored today) |
| **Size (< / > / within, +unit)** | **Missing** — `size` column exists (`sql.rs`), no field/operator | MailMate: size number+unit | MED |
| **List-Id / mailing-list** | **Missing** | MailMate: List-Id | LOW-MED |
| **`matches-regex` operator** | **Missing** | MailMate: regex per field | MED |
| **`begins/ends-with`** | **Missing** (only `contains`) | MailMate: begins/ends-with | LOW-MED |
| **`is-not` / negated operator** | Partial — via `negated` flag, not a first-class "is not" operator | MailMate: explicit is-not | LOW (semantically covered) |
| **Body full-text search** | Partial — FTS5 indexes preview only, not body (`sql.rs:326`) | MailMate: full body | MED |
| **is-in-address-book** | **Missing** | MailMate: address-book membership | LOW |
| **Keyword/tag value picker** | **Missing** — bare text (`ConditionEditor.tsx:199`) | MailMate: known-value list | MED |
| **Top-level OR in search grammar** | **Missing** — root is AND-only (`lib.rs:87`) | MailMate query OR | LOW (builder covers OR; grammar is a convenience) |

**Summary of the gap:** the *structure* (nesting/negation/operator-subsetting) is at parity or better; the *value-entry ergonomics* and a handful of *fields/operators* are where Posthaste trails. The owner's phrasing — "auto-filling the correct value format on creation" — points precisely at the type-directed value editor, which is an editor-only fix.

---

## 3. Recommendations (prioritized, opinionated)

### R1 — Type-directed condition editor (editor-only; HIGHEST VALUE) 🥇

Introduce a single **field descriptor registry** in the web app that drives the whole condition row from the chosen field. This is the direct answer to the owner's ask and touches **no Rust**.

Define, in `smartMailboxForms.ts` (or a new `fieldRegistry.ts`), one table keyed by `SmartMailboxField`:

```
fieldRegistry: Record<SmartMailboxField, {
  valueType: 'text' | 'boolean' | 'date' | 'mailboxRef' | 'accountRef'
           | 'roleEnum' | 'keyword' | 'address',
  operators: SmartMailboxOperator[],      // replaces operatorOptionsForField
  widget: (op) => ValueWidget,            // the type-directed part
  defaultValue: SmartMailboxValue,
}>
```

Then replace the two-branch `ValueEditor` (`ConditionEditor.tsx:168`) with a widget dispatch on `valueType` + operator:
- **`date` (`receivedAt`)** → a date/relative-date picker that emits the RFC3339 string the compiler expects (`field_compilers.rs:90`). Reuse the relative-date vocabulary already in `date.rs` (`Nd/Nw/Nm/Ny`) so "last 7 days" round-trips. **Biggest single ergonomic win.**
- **`mailboxRef` (`mailboxId`)** → the mailbox-picker `<Select>` that **already exists** at `ActionListEditor.tsx:192` — lift it into a shared component and reuse. (Also fixes the System-A `move`-action text box, `RuleActionEditor.tsx:120`.)
- **`accountRef` (`sourceId`)** → an account picker from the accounts the client already lists.
- **`roleEnum` (`mailboxRole`)** → a role `<Select>` (the same `ASSIGNABLE_MAILBOX_ROLES` used in `SmartMailboxEditor.tsx:176`).
- **`keyword`** → tag autocomplete against known tags (the app already has tag summaries).
- **`address` (`fromEmail`/`fromName`)** → address autocomplete against `sender_address_cache` (`sql.rs:190`) — surface a small completions endpoint or reuse existing sender lookups.
- **`boolean`** → keep the existing `true/false` select.
- **`in` operator** → a proper multi-chip input instead of comma-splitting one text box.
- **`text`** → the generic input (the honest fallback).

Also: **add `conversationId` to `FIELD_OPTIONS`** (or consciously drop it) so model and UI agree; align the hand-authored `SmartMailboxField` union with the generated schema.

**Layer:** web only (`ConditionEditor.tsx`, `smartMailboxForms.ts`, new registry + a couple of picker components). **Size:** M (~1–2 days; the pickers mostly exist). **Risk:** LOW — no wire/model change; the emitted `SmartMailboxValue` is unchanged (still string/strings/bool), so every existing rule/query keeps working. This is the cheap, high-value first step.

### R2 — Add recipient fields: To/Cc/Bcc (model + evaluator + editor) 🥈

The most-missed *field* gap. Data already lives in `message.to_json` (`sql.rs`). Cc/Bcc are not currently columns — scope decision: (a) ship `ToEmail`/`ToName` now over `to_json` and defer Cc/Bcc until those are projected, or (b) project recipients into a `message_recipient(kind, email, name)` table and expose `To/Cc/Bcc/AnyRecipient`. Recommend (a) first.

Add `SmartMailboxField::ToEmail`/`ToName` (`smart_mailboxes.rs:43`), a `compile_*` arm (`rule_compiler.rs:53` + a `to_json`-aware text/EXISTS compiler in `field_compilers.rs`), grammar prefixes `to:`/`cc:` (`nodes.rs:12`), and registry entries (R1). **Layer:** Rust model + store + grammar + web. **Size:** M (option a) / L (option b). **Risk:** MED — new SQL over a JSON column; index carefully.

### R3 — Add `Size` field + numeric operators (model + evaluator + editor)

`message.size` column exists (`sql.rs`). Add `SmartMailboxField::Size`, reuse `Before/After/OnOrBefore/OnOrAfter` as numeric `< > <= >=` (the date compiler's comparators generalize) OR add explicit `LessThan/GreaterThan` operators. Editor widget: number + unit (KB/MB) that emits bytes. **Layer:** Rust + web. **Size:** S–M. **Risk:** LOW.

### R4 — Add `matches-regex` + `begins-with`/`ends-with` operators

Extend `SmartMailboxOperator` (`smart_mailboxes.rs:69`) and the text compiler (`field_compilers.rs:59`): begins/ends map to `LIKE 'x%'`/`'%x'`; regex maps to SQLite `REGEXP` (needs a registered function) or is evaluated in Rust for the single-row rules path. Editor: a validated pattern box for regex (compile-check on blur). **Layer:** Rust + web. **Size:** M (regex needs a SQLite function hookup). **Risk:** MED — regex over a full smart-mailbox scan is a performance footgun; gate regex to the rules single-row path first, or document the cost.

### R5 — Do NOT add a boolean-nesting model change — it already exists

Call this out explicitly so no one "adds" it: nested AND/OR/NOT is present in model, evaluator, and editor (§1.1, §1.2, §1.5). No schema change is warranted here. The only grammar-level gap is top-level `OR`/parens in the **text** search bar (`lib.rs:87`) — a parser enhancement, LOW priority, independent of the editor.

### R6 — Action set: leave as-is; unify the two systems later (SECURITY NOTE)

Action coverage is adequate for beta (tag/move/notify/emit/webhook in System A; the mark/flag/move set in System B). The real debt is **two parallel rule systems** (§1.4) with divergent action editors — a consolidation project, out of scope for this review. **Security constraint (restated):** whatever consolidation happens, `RuleAction::Exec` stays config-file-only and must never be added to `WritableRuleAction` or any REST write body (`rules.rs:156`, RFC-L2-scripting ruling 23) — a GUI/REST-settable exec is remote code execution. Do not expose exec to the editor or API.

---

## 4. Sequencing

1. **R1 — type-directed condition editor (web-only).** Cheap, highest user-visible payoff, zero model/wire risk, directly satisfies "auto-fill the correct value format on creation." Ship first. Fold in the `conversationId`/schema alignment and the System-A move-action mailbox picker as free riders.
2. **R3 — Size field + numeric ops.** Small, self-contained, column already present; good second step to validate the "add a field end-to-end" path with low risk.
3. **R2 — To/Cc/Bcc recipient fields.** Higher value than Size but more work (JSON/recipient projection); do it once the R1 registry exists so the new fields get pickers for free.
4. **R4 — regex / begins-/ends-with operators.** Do last; regex carries a real performance/perf-review cost — land it gated to the rules single-row path, and only broaden to smart-mailbox scans behind a measured decision.
5. **Explicitly skip** a boolean-nesting model change (R5, already exists) and any exec-over-REST exposure (R6, security-load-bearing).

The through-line: **the model and evaluator are ahead of the editor.** The owner's complaint is an editor problem first. Ship R1, then let the field/operator additions ride the registry R1 establishes.
