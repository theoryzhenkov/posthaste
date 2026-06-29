# LOC-Reduction Audit — link-core / link-contract / link-replica / link-wasm / testkit / config / observability

Scope: `crates/posthaste-link-core`, `posthaste-link-contract`, `posthaste-link-replica`,
`posthaste-link-wasm`, `posthaste-testkit`, `posthaste-config`, `posthaste-observability`
(~13k LOC Rust). Audit goal is **LOC reduction for AI-context fit**, not correctness.
Evidence gathered 2026-06-29 against the `reduce-loc` workspace.

Baseline (prod + test, `wc -l`):

| crate | total LOC | notable file |
|---|---|---|
| link-contract | 1543 | `lib.rs` 1293 |
| link-replica | 1799 | `entity_store.rs` 1774 (≈889 prod / ≈885 test) |
| link-core | 1268 | `convergence.rs` 666, `message.rs` 574 |
| link-wasm | 634 | `entity_store.rs` 460 |
| testkit | 2459 | `runtime.rs` 762, `gmail.rs` 600 |
| config | 2440 | `config_repository.rs` 260, `source_conversions.rs` 241, `smart_conversions.rs` 219 |
| observability | 336 | `events.rs` 197, `lib.rs` 130 |

---

## Findings

### F1 — `BackendApi` op list is written THREE times (macro table + trait defaults + `BackendLink` delegation)
- **category:** DUP / BOILERPLATE
- **file:** `crates/posthaste-link-contract/src/lib.rs` — `for_each_link_op!` table `1000-1146`; `BackendApi` trait default-erroring methods `289-654`; `BackendLink` delegation impl `685-985`
- **evidence:** The same ~37 ops appear in all three places. The trait methods are mechanical `let _ = (args); Err(write_channel_unsupported())` stubs (e.g. `425-433`, `607-613`). The `BackendLink` methods are pure pass-throughs `self.transport.<method>(args).await` (e.g. `762-771`, `936-941`). The authoritative list already exists as the `for_each_link_op!` x-macro, which today only emits the *request structs* (`define_link_request_structs`, `1164-1173`).
- **how:** Add two emitter macros consuming `for_each_link_op!` — one emitting the `BackendApi` default-erroring methods, one emitting the `BackendLink` delegations — exactly as `define_link_request_structs` already does for the structs. Keep the 6 bespoke methods (`forward_mutation`, `subscribe`, and the 4 read-channel methods `query_mail_page`/`current_summary`/`message_detail`/`conversation` that are not in the table) hand-written; optionally add those 4 reads as table rows to fold them in too.
- **EST_LOC_SAVED:** ~430 (≈300 from `BackendLink`, ≈300 from trait defaults, minus ~50 for two emitter macros + the ~6 retained bespoke methods + lost doc-comments). Conservative floor ~350.
- **risk:** med / behavior-change: **n** — generated bodies are byte-identical to the hand-written ones; main cost is loss of per-method doc comments (acceptable for context-fit) and macro-emitter authoring care.

### F2 — `replica_probe.rs` is fully dead (module + re-export)
- **category:** DEAD
- **file:** `crates/posthaste-testkit/src/replica_probe.rs` (118 LOC) + `lib.rs:18` (`mod replica_probe;`) + `lib.rs:26` (`pub use replica_probe::{FlickerLog, RenderSnapshot, RenderedRow};`)
- **evidence:** Module doc states the `ReplicaProbe` it served is "**retired**". `grep` for `FlickerLog {` / `RenderSnapshot {` / `assert_no_flicker` across all `*.rs` returns **zero** constructions/calls outside the file itself; the only other references are doc-comment links in `runtime.rs:382,417`.
- **how:** Delete the file + the `mod`/`pub use` lines; drop the two doc-link references in `runtime.rs`.
- **EST_LOC_SAVED:** ~122
- **risk:** low / behavior-change: **n**

### F3 — `open_capture` + `FrameCapture` are dead testkit API
- **category:** DEAD
- **file:** `crates/posthaste-testkit/src/runtime.rs:375-448` (method `open_capture` + struct/impl `FrameCapture`), exported at `lib.rs:27`
- **evidence:** `grep open_capture\|FrameCapture` finds only the definition, the `lib.rs` re-export, and self-referential doc comments — **no caller** in any test.
- **how:** Remove the method, the `FrameCapture` struct+impl, and trim the `lib.rs` re-export.
- **EST_LOC_SAVED:** ~70
- **risk:** low / behavior-change: **n**

### F4 — Toml↔domain enum mirrors mapped by hand in both directions
- **category:** BOILERPLATE / DUP
- **file:** `crates/posthaste-config/src/schema/smart_conversions.rs` (219, 69 `=>` arms) + `source_conversions.rs` (241, 34 arms); mirror enums in `schema/smart_types.rs` (`FieldToml`, `ConditionOperatorToml`, `GroupOperatorToml`, `SmartMailboxKindToml`) and `source_types.rs` (`DriverToml`, `SecretKindToml`, …)
- **evidence:** Each `*Toml` enum is 1:1 with a domain enum with identically-named variants, mapped with hand-written `match` arms *twice* (to-domain and from-domain), e.g. `FieldToml`→`SmartMailboxField` is 17 arms in each direction (`smart_conversions.rs:88-105`), plus `ConditionOperatorToml` 7×2, `DriverToml` 3×2, `SecretKindToml` 2×2, etc.
- **how:** Introduce a small `bimap_enum!(TomlEnum <=> DomainEnum { VariantA, VariantB, … })` macro emitting both `From`/`From` arms from one variant list, and use it for the ~6 trivially-isomorphic enum pairs. (The deeper win — deleting the `*Toml` mirror enums entirely by deriving serde on the domain enums — is higher risk and touches the out-of-scope `posthaste-domain` crate; left as a note.)
- **EST_LOC_SAVED:** ~120 (of the ~460 conversion lines; struct-field copies and value parsing stay)
- **risk:** med / behavior-change: **n** (mechanical; the macro must preserve the exact variant correspondence)

### F5 — 12 unused `LogEvent` constants
- **category:** DEAD
- **file:** `crates/posthaste-observability/src/events.rs`
- **evidence:** Repo-wide `grep` (incl. `apps/`) finds zero `ph_*!` uses of: `API_REQUEST_COMPLETED`, `API_REQUEST_FAILED`, `API_REQUEST_STARTED`, `CONFIG_INITIALIZED`, `DAEMON_EVENT_MALFORMED`, `FRONTEND_CONSOLE_OUTPUT`, `FRONTEND_ERROR_UNCAUGHT`, `FRONTEND_ERROR_UNHANDLED_REJECTION`, `SEND_FOLLOWUP_SYNC_TRIGGER_FAILED`, `DRAFT_FOLLOWUP_SYNC_TRIGGER_FAILED`, `SUPERVISOR_OUTBOX_FLUSH_FAILED`, `SUPERVISOR_SYNC_TRIGGER_IGNORED`.
- **how:** Delete the 12 `pub const` declarations (several span 2 lines).
- **EST_LOC_SAVED:** ~18
- **risk:** low / behavior-change: **n** (verify none are referenced from non-Rust dashboards before deleting; they are plain consts so unlikely)

### F6 — `ph_*!` log macros are 5 near-identical copies
- **category:** BOILERPLATE
- **file:** `crates/posthaste-observability/src/lib.rs:27-90` (`ph_trace`/`ph_debug`/`ph_info`/`ph_warn`/`ph_error`)
- **evidence:** The five macros differ only by the inner `tracing::<level>!` name; each has the identical 3-arm (`target:` / `parent:` / bare) body (~12 lines each, ~63 total).
- **how:** Generate them with a single `define_ph_macro!(ph_debug => debug);`-style meta-macro. Note: nested `macro_rules!` with `$($fields)+` requires `$$`-style escaping discipline; modest authoring cost.
- **EST_LOC_SAVED:** ~35 (63 → ~28)
- **risk:** med / behavior-change: **n** (macro hygiene is fiddly; the 5 `ph_forwarded_*` macros are genuinely used by `apps/desktop` — leave them)

### F7 — Stale `MailListReplica` doc references (no longer exists)
- **category:** DEAD (doc) — minor
- **file:** `crates/posthaste-link-replica/src/entity_store.rs:3`, `crates/posthaste-link-wasm/src/entity_store.rs:5`, `crates/posthaste-runtime/src/near_node.rs:14`
- **evidence:** `grep` for `pub struct MailListReplica` / `struct MailListReplica` returns **nothing** — the type is gone; only doc-comment back-references remain (`[crate::MailListReplica]`, `MailListReplicaHandle`).
- **how:** Reword the three doc lines (no rustdoc broken-link). Trivial; aggregated here only for hygiene.
- **EST_LOC_SAVED:** ~0 (doc fix; flagged so the stale refs don't mislead)
- **risk:** low / behavior-change: **n**

---

## Cross-language duplication (noted, mostly out of Rust scope)

`review-unification.md` §3.1–§3.3 documents the largest single duplication: the TS client
replica (`apps/web/src/runtime/replica/replicaAdapter.ts`, `handle.ts`) reimplements the
fold/diff/assertion logic that **already** lives centralized in Rust:
- `posthaste-link-contract/src/message_mutation.rs` (`MessageMutation::to_assertion`, `159` lines) is the single Rust mutation-name→assertion table.
- `posthaste-link-core/src/message.rs` (`MessageChangeDiff`, `KeywordDelta`, `inverse()`).
- `posthaste-link-replica/src/entity_store.rs` + `posthaste-link-wasm` already expose the fold engine across WASM.

The Rust side is **not** duplicated (entity_store correctly delegates to link-core's
`Replica<MessageConvergence>`). The win is deleting the TS reimplementation in favour of the
WASM handle — large LOC reduction but in `apps/web` (out of this scope). Called out so the
cross-scope owner can claim it.

Likewise `review-unification.md` §8.1 (`temp_root()` + `DatabaseStore::open` copied ~40×) is
**already centralized inside this scope** — `posthaste-testkit/src/paths.rs:temp_root` and
`harness.rs` are the canonical helpers; the duplication lives in `posthaste-store` /
`posthaste-server` test modules (out of scope). No action here beyond noting testkit is the
home those crates should adopt.

---

## Test-bloat observation (low-confidence, no recommendation)

`entity_store.rs` (885 test LOC), `convergence.rs` (≈290), `message.rs` (≈270) carry large
but high-value replication-invariant suites (out-of-order settle, absorption-retire, stale
re-serve, version-hold). These guard the exact bugs in the memory index (flicker /
absorption-retire). **Not** recommended for reduction — listed for completeness only.

---

## Totals

| ID | save | risk |
|---|---|---|
| F1 BackendApi triple-list → emitter macros | ~430 | med |
| F4 Toml↔domain enum bimap macro | ~120 | med |
| F2 dead `replica_probe.rs` | ~122 | low |
| F3 dead `open_capture`/`FrameCapture` | ~70 | low |
| F6 `ph_*!` meta-macro | ~35 | med |
| F5 12 unused `LogEvent` consts | ~18 | low |
| F7 stale doc refs | ~0 | low |
| **TOTAL (in-scope)** | **~795** | |

Plus cross-scope (not counted): TS replica/diff/assertion reimplementation (large, `apps/web`).

## Top 5 by LOC/risk

1. **F1 — collapse the `BackendApi`/`BackendLink` op list into emitter macros over the existing `for_each_link_op!` table (~430 LOC, med/n).** Biggest single win; the unification mechanism already exists and is praised in `review-unification.md` — this just extends it from request-structs to the trait+delegation.
2. **F2 — delete dead `replica_probe.rs` (~122 LOC, low/n).** Self-documented as retired; zero callers. Pure deletion.
3. **F3 — delete dead `open_capture`/`FrameCapture` testkit API (~70 LOC, low/n).** Zero callers; pure deletion.
4. **F4 — `bimap_enum!` for the 1:1 Toml↔domain enums (~120 LOC, med/n).** Mechanical; halves the largest config boilerplate without touching the schema boundary.
5. **F5 — drop 12 unused `LogEvent` consts (~18 LOC, low/n).** Trivial, safe, verified repo-wide.
