# LOC-Reduction Audit — `posthaste-api` + `posthaste-server`

Scope: `crates/posthaste-api/` (10,086 LOC src) + `crates/posthaste-server/`
(8,735 LOC, mostly tests). Goal is **fewer lines for AI context**, not
correctness. Evidence gathered 2026-06-29 from the `reduce-loc` workspace.

Headline: the single biggest context win is **not** in `.rs` at all — it's the
7,715-line generated `openapi.json` (plus the 150-line `asyncapi.json`). Those
are build artifacts and should be *excluded from context*, not hand-trimmed.

---

## Findings

| ID | Category | File:line(s) | EST_LOC_SAVED | Risk / behav-change | How |
|----|----------|--------------|---------------|---------------------|-----|
| **C1** | CONTEXT-EXCLUDE | `openapi.json` (root), embedded via `api/src/openapi.rs:262` (`include_str!` `asyncapi.json`) | **~7,865** (excl. from context) | low / **n** | Generated from utoipa `#[derive(OpenApi)]`. Add to context-ignore (`.aiignore`/tooling) — never hand-edit; it regenerates. Not source LOC. |
| **D1** | DEAD | `api/src/auth/perimeter.rs:11-31` (`constant_time_eq` + `#[allow(dead_code)]`) and its unit test in `auth/tests.rs` | ~30 | low / n | Re-exported (`auth.rs:30`) but no production caller; only a unit test exercises it. Token gate is now macaroon HMAC. Delete fn + comment + test, drop the re-export. (Matches review-fat-legacy §4.) |
| **DUP1** | DUP | `api/src/api/errors.rs:13-235` (`ApiErrorCode` enum + `From<ServiceErrorKind>` + `runtime_error_status_code` + `service_error_status`) vs `posthaste-runtime-contract` `RuntimeErrorCode` | ~70 | med / n (careful) | `ApiErrorCode` is a near-superset of `RuntimeErrorCode`; three hand-written mapping tables (~150 lines) restate the same domain concepts. Collapse to one shared code enum + a single status table; keep only the boundary-only codes (`InvalidQuery`, `InvalidCursor`, …) as an API extension. (review-unification §5.1.) |
| **DUP2** | DUP / TEST-BLOAT | `server/tests/*/support.rs` ×8 (`temp_root` in 9 files; `TestSecretStore`+`SecretStore` impl identical in `auth_middleware/support.rs:46`, `support/mod.rs:58`, `api_boundary_contracts/support.rs:33`, `settings_patch/support.rs:32`, `capability_scoping/support.rs:43`, `automation_preview.rs:37`; plus `secret_key`/`test_root_key`) | ~150 | low / n | No shared test-support crate. The no-op `TestSecretStore` (~18 lines) is byte-identical across 5+ files; `temp_root` (~9 lines) across 9 (only the temp-dir name-prefix differs). Hoist into one `posthaste-test-support` dev-crate (or a single `tests/common`) with a parameterized `temp_root(prefix)`. (review-unification §8.1.) |
| **T1** | TEST-BLOAT | `server/tests/runtime_wrapper.rs:480-727` (6 source-grep "fitness" tests: `account_asset_routes…`, `app_state_does_not_expose…`, `migrated_runtime_routes…`, `authority_runtime_core…`, `runtime_contract_exposes…`, `api_route_modules…` + the recursive `collect_forbidden_runtime_graph_constructors` walker) | ~150 | med / n | These read other crates' source as strings and assert substring-absence — migration guards for `PLAN-L3-api-runtime-wrapper-migration`. They are *not* behavioral tests. When that migration lands (review-fat-legacy §2/§8), delete them; until then they are dead-weight to a reader. Flag, don't remove yet. |
| **T2** | TEST-BLOAT | `server/tests/backend_link_split.rs:5 inline `TcpListener::bind`+`tokio::spawn(axum::serve)` blocks (lines ~200, ~290, ~420…) + 2 inline `SyncBatch` seed blocks duplicating `seed_inbox_message` | ~40 | low / n | A `serve_link` helper already exists (line ~150) but 3 tests inline the 6-line serve dance; 2 tests inline a full 25-line `apply_sync_batch` that `seed_inbox_message` already encapsulates. Route the inliners through the helpers. |
| **B1** | BOILERPLATE | `api/src/api/message_commands.rs:1-188` (5 handlers) | ~50 | med / n | Each handler is a ~15-line `#[utoipa::path]` block + a ~12-line body that only `state.runtime.<verb>(RuntimeCaller::api(), AccountId, MessageId, command).map(Json).map_err(ApiError::from_runtime_error)`. A `forward_message_command!` macro could collapse the 5 bodies. **Caveat:** macro-hiding the `#[utoipa::path]` breaks OpenAPI generation, so only the *bodies* (~5×8 lines) are safely macro-able. Medium value, obscures readability — borderline. |
| **A1** | ARCH / DUP (flag only) | route fan-out: `api/src/router.rs` (52 `.route()`), `api/src/authz/route_table/*` (60 `Entry{}`, 475 LOC), 60 `#[utoipa::path]` blocks across `api/`, and `openapi.rs:30-89` `paths(...)` list | ~0 recommended | high / y if unified | Every route path is restated up to 4× (axum wiring, authz map, handler annotation, openapi aggregator). Real ~400+ LOC of restated templates, but each enumeration is load-bearing (authz map is a security artifact cross-checked in CI; utoipa needs per-handler annotation). Unifying needs a route-descriptor macro — large, risky rework. **Documented, not recommended for a LOC pass.** |
| **N1** | NITPICK (aggregate) | `api/src/api.rs:14-30` + `:111-117` two `#[allow(unused_imports)]` glob blocks | ~5 | low / n | The big `use posthaste_domain::{…}` block is re-exported to submodules via `use super::*`; a few names are genuinely unused. Marginal; only worth it alongside a broader import cleanup. |

---

## Totals

- **Hand-source LOC removable now (low/med risk):** D1 30 + DUP2 150 + T2 40 +
  N1 5 ≈ **225 LOC** with low risk; +DUP1 70 + T1 150 + B1 50 ≈ **270 LOC**
  more at medium risk / pending-migration → **~495 LOC** total addressable.
- **Context-excludable (not source):** C1 ≈ **7,865 lines** of generated JSON —
  by far the largest context reduction, achieved by tooling config, zero risk.

## Top 5 ranked by LOC ÷ risk

1. **C1 — exclude `openapi.json`/`asyncapi.json` from context** (~7,865 lines, ~0 risk). Pure tooling config; dwarfs everything else.
2. **DUP2 — consolidate test-support (`temp_root`/`TestSecretStore`)** (~150 LOC, low risk). Mechanical hoist into one dev-crate.
3. **D1 — delete `constant_time_eq` + test** (~30 LOC, low risk). Confirmed dead by review-fat-legacy §4.
4. **T2 — route `backend_link_split` inliners through existing helpers** (~40 LOC, low risk). Helper already exists.
5. **DUP1 — unify `ApiErrorCode`/`RuntimeErrorCode` tables** (~70 LOC, med risk). Highest single-file source win but cross-crate; do deliberately.

**Deliberately deferred:** T1 (delete when the L3 wrapper migration lands), A1
(route fan-out — needs a descriptor macro; high risk, not a LOC-pass item),
B1 (macro only the handler bodies, keep the `#[utoipa::path]` annotations).
