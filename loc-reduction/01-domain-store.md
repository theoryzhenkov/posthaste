# LOC-Reduction Audit — `posthaste-domain` + `posthaste-store`

Scope: `crates/posthaste-domain/` (~15.6k LOC) and `crates/posthaste-store/`
(~11.6k LOC). Goal is shrinking source so the codebase fits AI context better.
This is a LOC-reduction audit, not a correctness review. Evidence cited as
`file:line`. Findings <10 LOC are skipped unless they aggregate.

Format: `ID | category | file:line(s) | EST_LOC_SAVED | risk + behavior-change | how`

---

## Findings

### D1 — SyncBatch full literals can use `..Default::default()`
**category:** VERBOSE / BOILERPLATE
**files:** `posthaste-domain/src/service/outbox.rs:75-105` (`upsert_message_batch`,
`delete_message_batch`); `service/tests/message_mutation_retries.rs:411-426`
(`observe_batch`); and ~28 full `SyncBatch { … }` literals across
`posthaste-store/src/tests/*` and `posthaste-domain/src/service/tests/*`
(grep `replace_all_messages: false` → 28 hits in scope; `imap_mailbox_states: Vec::new()`
→ 34 hits in scope).
**EST_LOC_SAVED:** 150
**risk:** low / behavior-change: n
**how:** `SyncBatch` already `#[derive(Default)]` (`model/records.rs:121`). Every
full literal spells out all 10 fields, but most set only 1–2. Replace the
zero-valued fields with `..Default::default()`. The two production helpers in
`outbox.rs` collapse from ~12 lines each to ~4; each test literal drops ~6–8
lines. Purely mechanical, identical runtime value.

### D2 — Store outbox enum⇄string tables duplicate the domain serde reprs
**category:** DUP / BOILERPLATE
**files:** `posthaste-store/src/outbox.rs:7-74` — `parse_operation_state`,
`operation_state_str`, `parse_operation_kind`, `operation_kind_str`,
`parse_entity_kind`, `entity_kind_str` (6 fns, ~68 LOC).
**EST_LOC_SAVED:** 45
**risk:** low / behavior-change: n
**how:** The DB strings are *exactly* the domain enums' camelCase serde reprs
(`OperationKind`/`OperationState`/`OperationEntityKind` already derive
`Serialize`/`Deserialize` with `rename_all = "camelCase"`, `model/outbox.rs:30/55/127`):
`"setKeywords"`, `"pending"`, `"message"`, etc. Implement `Display`+`FromStr`
on the domain enums (via `serde_plain` or a tiny macro) and delete the six store
functions, replacing call sites with `state.to_string()` / `OperationState::from_str(s)?`.
Keep one explicit branch for the legacy `"conflicted" => Pending` recovery
(`outbox.rs:14-16`) so that compatibility path survives. (Matches
`review-unification.md` §2.1.)

### D3 — Repeated payload (de)serialize + `GatewayError::Rejected(format!)` map_err
**category:** DUP / BOILERPLATE
**files:** `posthaste-domain/src/service/mutation.rs:104-129` (3×),
`service/message_queries.rs:300-330` (2×), `service/outbox.rs:236-244,300-308,
…` and `merge_set_keywords:790-822`; ~11 `GatewayError::Rejected(format!` map_err
blocks wrapping 30 `serde_json::{to_value,from_value}` calls in the crate.
**EST_LOC_SAVED:** 45
**risk:** low / behavior-change: n
**how:** Two helpers — `fn encode_payload<T: Serialize>(v: &T, ctx: &str) ->
Result<Value, ServiceError>` and `fn decode_payload<T: DeserializeOwned>(v:
&Value, ctx: &str) -> Result<T, ServiceError>` — each collapse a ~5-line
`.map_err(|error| ServiceError::from(GatewayError::Rejected(format!("…: {error}"))))`
block to one line. `parse_payload` in `outbox.rs:773` already does this for the
`FlushError` variant; generalize it.

### D4 — Test setup quadruple + command builders duplicated in service tests
**category:** TEST-BLOAT
**files:** `posthaste-domain/src/service/tests/outbox.rs` and
`tests/message_mutation_retries.rs` (55 `MailService::new` call sites across
`service/tests`); `message_mutation_retries.rs` builds the same
`ReplaceMailboxesCommand { mailbox_ids: vec![MailboxId::from("archive")] }` 13×
and `SetKeywordsCommand { add: vec!["$flagged"…], remove: Vec::new() }` repeatedly.
**EST_LOC_SAVED:** 120
**risk:** low / behavior-change: n
**how:** Add fixtures to `service/tests/fixtures.rs`: `fn service_with(state) ->
(AccountId, Arc<TestStore>, MailService)` (collapses the repeated 3–4 line
`account/store/service` preamble) and thin builders
`async fn do_replace(service, &acct, id, mailbox)` / `do_set_keywords(...)`. Each
test sheds ~6–10 lines. No coverage change — same calls, fewer literals.

### D5 — `query_message_page` / `query_conversations` share pagination scaffolding
**category:** DUP
**files:** `posthaste-store/src/smart_mailboxes/messages.rs:55-170` vs
`smart_mailboxes/conversations.rs:70-260`. Identical helpers exist in both:
`seek_op`/`dir` match on `SortDirection` (messages.rs:66-73, 178-181;
conversations.rs:84-91), `is_numeric*` + `*cursor_sort_sql_value`
(messages.rs:222-248 vs conversations.rs:48-67), and the
`rusqlite::types::Value → String` conversion (messages.rs:259-266 duplicated
inline at conversations.rs:237-243).
**EST_LOC_SAVED:** 35
**risk:** low / behavior-change: n
**how:** Lift shared helpers into the `smart_mailboxes` module: `fn seek_sql(dir:
SortDirection) -> (&'static str, &'static str)` (seek op + ORDER dir), and one
`fn sql_value_to_cursor_string(&Value) -> String`. Generic-over-sort-field
`cursor_sort_sql_value` can take an `is_numeric: bool`. The big SQL strings stay
distinct; only the surrounding boilerplate collapses.

### D6 — `field_compilers.rs` repeats the "unsupported operator" error 5×
**category:** DUP
**files:** `posthaste-store/src/smart_mailboxes/field_compilers.rs:51-55,84-88,
107-113,121-125,160-165` (same 4-line
`StoreError::Failure(format!("unsupported operator {:?} for field {:?}", …))`).
**EST_LOC_SAVED:** 18
**risk:** low / behavior-change: n
**how:** `fn unsupported_operator(c: &SmartMailboxCondition) -> StoreError`
helper; each site becomes `return Err(unsupported_operator(condition))` /
`_ => Err(unsupported_operator(condition))`.

### D7 — `ValidationError` carries dual hand-written `message()` + `Display`
**category:** BOILERPLATE / VERBOSE
**files:** `posthaste-domain/src/validation.rs:22-53` (`message()` and `Display`
both pattern-match all 8 variants); plus ~14 repeated
`errors.push(ValidationError::InvalidAccount("…".to_string()))` calls
(`validation.rs:160-260`).
**EST_LOC_SAVED:** 25
**risk:** low / behavior-change: n (msg text identical)
**how:** Drop `message()` if only `Display` is needed (check callers — it appears
internal), or generate both via `thiserror`/`strum`. Add a private
`fn invalid(errors: &mut Vec<_>, msg: &str)` to collapse the
`push(InvalidAccount(... .to_string()))` repetitions to one-liners.

### D8 — `flush_account` repeats the fail-and-settle block 3×
**category:** DUP
**files:** `posthaste-domain/src/service/outbox.rs:378-470` — the
`update_operation_state(Failed) → OperationSettlement{…Failed…} → emit_settlement
→ emit_failure_base_correction` sequence appears for the send-interrupt path
(388-411), the dependency-cancelled path (419-440), and the
`FlushError::Permanent` path (497-516).
**EST_LOC_SAVED:** 30
**risk:** med / behavior-change: n
**how:** Extract `fn fail_operation(&self, acct, &op, msg) -> Result<Vec<DomainEvent>>`
that does the state write + settlement + base correction and returns the events;
each site becomes `events.extend(self.fail_operation(account_id, &operation,
message)?);`. Medium risk only because it touches the flush control flow — verify
the `break` vs `continue` semantics are preserved at each site.

### D9 — `replace_mailboxes` event payload builds the mailbox-id list twice
**category:** DUP
**files:** `posthaste-domain/src/service/mutation.rs:185-198` — `"mailboxIds"`
and `"arrivedMailboxIds"` are the identical
`command.mailbox_ids.iter().map(MailboxId::as_str).collect::<Vec<_>>()`.
**EST_LOC_SAVED:** 6
**risk:** low / behavior-change: n
**how:** Bind `let ids = …collect::<Vec<_>>();` once and reference it for both
JSON keys.

### D10 — `lock_read_pool` / `lock_write_connection` are the same poison-recovery
**category:** DUP
**files:** `posthaste-store/src/store.rs:62-81` (two near-identical fns differing
only in the warn message + guard type).
**EST_LOC_SAVED:** 10
**risk:** low / behavior-change: n
**how:** One generic `fn lock_recover<T>(m: &Mutex<T>, what: &str) ->
MutexGuard<'_, T>`; call sites pass the label.

---

## Notes (looked at, not worth trimming)

- **`model/*.rs` struct definitions** are mostly `#[derive]` + doc-comments that
  double as the SPECial `@spec` anchors — high doc density but low redundancy.
  Stripping docs would hurt the spec traceability the repo relies on; not
  recommended as LOC reduction.
- **`db/schema/sql.rs:1-387`** is one SQL string constant. It's long but
  irreducible content (table/index/trigger DDL); no duplication to collapse.
- **`.map_err(sql_to_store_error)?`** appears 257× in the store. Idiomatic and
  already minimal; a macro would obscure more than it saves. Skip.
- **`string_id!` macro** (`model/mod.rs:17-51`) is already the right
  consolidation for the 7 ID newtypes. Good.
- No `#[allow(dead_code)]`, `todo!()`, `unimplemented!()`, or commented-out code
  blocks in scope. Two `TODO(S3)` comments in `service/sync_ops.rs:29,392` are
  live design notes, not dead weight.

---

## (a) TOTAL estimated LOC saved

**~480 LOC** (D1 150 + D2 45 + D3 45 + D4 120 + D5 35 + D6 18 + D7 25 + D8 30 +
D9 6 + D10 10).

## (b) Top 5 by LOC-saved-per-risk

1. **D1 — SyncBatch `..Default::default()`** — 150 LOC, low risk, zero behavior
   change. Purely mechanical, the derive already exists. Highest absolute and
   highest leverage.
2. **D4 — Shared test fixtures/builders** — 120 LOC, low risk (tests only), no
   coverage loss. Two large test files carry most of the duplication.
3. **D2 — Outbox enum⇄string via Display/FromStr** — 45 LOC, low risk, identical
   strings. Deletes a whole translation layer and removes a model↔store drift
   surface.
4. **D3 — `encode_payload`/`decode_payload` helpers** — 45 LOC, low risk, no
   behavior change. Broad, repetitive, trivially safe.
5. **D5 — Pagination helper de-dup in `smart_mailboxes`** — 35 LOC, low risk.
   Collapses copy-pasted sort/cursor plumbing between the message and
   conversation query files.
