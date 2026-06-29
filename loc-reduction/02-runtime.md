# LOC-Reduction Audit — Runtime crates

> Scope: `crates/posthaste-authority-runtime/`, `crates/posthaste-runtime/`,
> `crates/posthaste-runtime-contract/`, `crates/posthaste-runtimed/` (~18.4k LOC).
> Goal: shrink lines so the subsystem fits AI context better. Not a correctness review.
> Date: 2026-06-29. No source files were edited.

## Orientation — what's already been done in this workspace

The `review-summary.md` / `review-unification.md` targets **U1** and **U2** are
largely *already implemented* in this workspace, so they are **not** LOC
opportunities here:

- **U1 (rename `AuthorityRuntime*` → `Runtime*`)** is done: `posthaste-runtime/src/build.rs`
  now defines `RuntimeBuildConfig`, `RuntimeHandle`, `RuntimeCore`, etc. (no `Authority`
  prefix on near-node types).
- **U2 (near vs far mutation dispatch table)** is done: both the near-node
  `named_message_assertion` (`near_node.rs:91-100`) and the far-node
  `apply_named_message_mutation` (`backend.rs:687-820`) now delegate to a single
  `MessageMutation::from_request` table in `posthaste-link-contract`.
- **`RemoteBackend`** (`transport.rs`) is **already macro-generated** from
  `for_each_link_op!`.
- No `#[allow(dead_code)]`, no `todo!()`/`unimplemented!()`, no commented-out code
  blocks anywhere in scope.

The real remaining fat is a **four-layer delegation pass-through**: the same
~35–45 operations are spelled out by hand in full, three to four times over.
That is where almost all of the recoverable LOC lives.

---

## The delegation pass-through (the headline)

One link op (e.g. `set_keywords`) is written out at **four** layers:

| Layer | File | Form | Status |
|---|---|---|---|
| op table (source of truth) | `link-contract/src/lib.rs:1001` `for_each_link_op!` | macro table | keep |
| `RemoteBackend` (near, remote) | `runtime/src/transport.rs:128` | **macro-generated** | already lean ✅ |
| `LocalBackend` (far, in-proc) | `authority-runtime/src/local_backend.rs:131-443` | **35 hand-written one-liners** | **DUP-1** |
| `RuntimeHandle: RuntimeCore` | `runtime/src/build.rs:612-1212` | **~45 hand-written delegations** | **DUP-2** |
| `RuntimeCore` trait def | `runtime-contract/src/lib.rs:977-1320` | 52 hand-written sigs | **DUP-3 (stretch)** |

`LocalBackend` and `RuntimeHandle` each re-state the op table that
`RemoteBackend` already generates from. Collapsing them onto the same (or a
sibling) macro table is the single biggest win in this subsystem.

---

## Findings

### DUP-1 | DUP | `crates/posthaste-authority-runtime/src/local_backend.rs:131-443` | EST_LOC_SAVED: 250 | med, behavior-change:n
`LocalBackend` hand-delegates 35 `BackendApi` methods to `self.backend.$method(...)`,
each an ~9-line block (`query_mail_page` … `reload_config`). `RemoteBackend` already
generates the equivalent surface from `for_each_link_op!`. Generate `LocalBackend`'s
read/typed-write delegations from the same table (the macro emits the whole
`#[async_trait] impl`, exactly as `remote_backend_impl!` does). The bespoke
`forward_mutation` (95-117), `subscribe` (444-478), and `message_event_to_assertion`
stay hand-written. *Risk note:* the underlying `Backend` methods aren't a clean
1:1 — some are sync (no `.await`), some take args by reference, `destroy_message`
maps to `Backend::destroy`, and `account_count` wraps in `Ok(...)`. The macro needs
a couple of per-row escape hatches (or a thin signature-aligned bridge), which is
why this is med not low. ~308 lines → ~55.

### DUP-2 | DUP/BOILERPLATE | `crates/posthaste-runtime/src/build.rs:612-1212` | EST_LOC_SAVED: 270 | med, behavior-change:n
`impl RuntimeCore for RuntimeHandle` is ~600 lines; `ensure_runtime_active()?`
appears **51 times**. ~37 of the ~45 methods are pure
`self.ensure_runtime_active()?; self.core.{reads|backend_link}.$method(args).await`
delegations (the `_caller` arg is dropped). Drive these from a declarative table
macro keyed by routing target, e.g.
`runtime_delegate!{ list_accounts => reads; patch_account(account_id, mutation) => backend_link; ... }`.
The non-trivial ~8 stay hand-written: `runtime_status` (count read), `get_account`
(`ok_or_else`), `get_message_detail` (wraps `CommandResult`), `subscribe_events`
(replay), `run_mutation`, `open_session`/view routing. ~37 × ~9 lines → table
+ macro ~60 LOC.

### DUP-3 | DUP | `crates/posthaste-runtime-contract/src/lib.rs:977-1320` | EST_LOC_SAVED: 230 | high, behavior-change:n
The `RuntimeCore` trait is ~343 lines of 52 method signatures that mirror
`BackendApi` plus a `_caller`/session/view overlay. If the op table were extended
to also emit the trait *signatures* (the same way `define_link_request_structs`
emits structs), the trait def and DUP-2's impl could be co-generated from one
list, so adding a link op touches one row instead of four. *Risk high:* this is a
near-public contract surface, `#[async_trait]` + `utoipa` interplay, and the
caller/session/view methods don't fit the plain op shape — they'd need a second
macro arm. Treat as a stretch; the LOC overlaps conceptually with DUP-2 (do DUP-2
first, then this if appetite remains).

### BOILERPLATE-1 | BOILERPLATE | `crates/posthaste-runtime-contract/src/lib.rs:841-895` | EST_LOC_SAVED: 30 | low, behavior-change:n
13 single-line `RuntimeError` constructors (`invalid_descriptor`, `invalid_mutation`,
`account_base_url_required`, `account_secret_required`, … each `Self::new(RuntimeErrorCode::X, message)`).
Collapse with a tiny declarative macro
(`error_ctors!{ invalid_descriptor => InvalidDescriptor, ... }`). The crate already
uses this idiom for ids (`define_id!`). ~52 lines → ~22. Low value but pure
mechanical, zero behavior change.

### TEST-BLOAT-1 | TEST-BLOAT | `crates/posthaste-authority-runtime/tests/authority_runtime_handle.rs` (2647 LOC, 34 tests) | EST_LOC_SAVED: 180 | med, behavior-change:n
~30 `#[tokio::test]`s each repeat the same arc: build runtime from temp roots →
seed a message batch → open a session → open a view → assert frames. Extract a
`TestRuntime` fixture (builder + `seed`/`open_mail_list`/`expect_frame` helpers)
to the file's helper block (or the proposed `posthaste-test-support` crate). The
seed builders (`seed_message_batch`, `seed_single_message_batch`,
`seed_heavy_body_message_batch`, `mail_list_descriptor*`) are already local
helpers — folding the per-test build+open+subscribe ceremony behind a fixture
trims ~5–7 lines/test. Conservative ~180. Med because it's a broad test rewrite
(easy to introduce flakiness if the harness over-abstracts timing).

### DUP-4 | DUP | `temp_root()` duplicated across the workspace (in-scope: `tests/authority_runtime_handle.rs:32`) | EST_LOC_SAVED: 35 | low, behavior-change:n
`fn temp_root()` + `DatabaseStore`/runtime setup is copy-pasted in ≥10 test files
(store, server, this crate). A `posthaste-test-support` crate (review-unification §8)
removes the in-scope copy plus the seed/account fixtures shared with the server
suites. Counted conservatively for the in-scope portion only; the workspace-wide
saving is larger but outside this audit's crates.

### DEAD-1 (BLOCKED) | DEAD | `crates/posthaste-authority-runtime/src/build.rs:50-72` + callers | EST_LOC_SAVED: 60 | high, behavior-change:y
`AuthorityRuntimeApiMigrationBridge` (struct + `new` + the four `pub` fields +
`from_api_bridge_*_for_migration` constructors) is explicit transitional debt
tagged `PLAN-L3-api-runtime-wrapper-migration#legacy-fields-temporary`. **Do not
remove yet** — it is consumed by `posthaste-server/src/migration.rs`, the bench,
and heavily by this crate's own tests; `runtime_wrapper.rs:640-659` asserts the
core does *not* depend on it. Listed for completeness/tracking; the ~60 LOC come
back only when the migration plan lands. Consider `#[deprecated]` on the migration
constructors now to stop new uses.

### VERBOSE-1 (no action) | VERBOSE | `crates/posthaste-runtime/src/build.rs` (1399), `sessions.rs` (1130), `views.rs` (823) | EST_LOC_SAVED: ~0 | — , —
Flagged by the task as oversized, but after inspection these are *substantive
logic*, not boilerplate: `SessionRegistry` (sessions.rs:132-668) and `ViewRegistry`
(views.rs:88-481) are single dense impl blocks doing real frame collapsing / delta
/ recompute work, with inline test modules. No safe mechanical reduction beyond
DUP-2's slice of build.rs. Recording so it isn't re-audited as "fat".

---

## Totals

| Bucket | EST_LOC_SAVED |
|---|---|
| Safe + realistic (DUP-1, DUP-2, BOILERPLATE-1, TEST-BLOAT-1, DUP-4) | **~765** |
| Stretch (DUP-3, if appetite after DUP-2) | +230 |
| Blocked (DEAD-1, when PLAN-L3 lands) | +60 |
| **Total addressable** | **~1,055** |

The ~765 "safe + realistic" figure is the number to plan around; ~520 of it
(DUP-1 + DUP-2) is the same one move — pull the two hand-written delegation
layers onto the existing `for_each_link_op!` machinery.

## Top 5 ranked by LOC-saved-per-risk

1. **DUP-2** — `RuntimeHandle: RuntimeCore` delegation macro — **270 LOC, med.** Highest absolute win at moderate risk; self-contained to one impl block; behavior-preserving table macro.
2. **DUP-1** — `LocalBackend` delegation macro from `for_each_link_op!` — **250 LOC, med.** Reuses the exact pattern `RemoteBackend` already proves; needs minor per-row escape hatches.
3. **TEST-BLOAT-1** — `TestRuntime` fixture in the 2647-line handle test — **180 LOC, med.** Pure test ergonomics, no production risk.
4. **BOILERPLATE-1** — `RuntimeError` constructor macro — **30 LOC, low.** Trivial, mechanical, mirrors existing `define_id!` idiom; near-zero risk.
5. **DUP-4** — shared `temp_root`/fixtures via `posthaste-test-support` — **35 LOC in-scope, low.** Low risk, and unlocks much larger workspace-wide savings outside scope.
