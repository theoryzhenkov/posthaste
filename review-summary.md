# Posthaste Codebase Review — Synthesis

> Date: 2026-06-26
> Scope: full workspace `crates/`, `apps/`, `tools/`, `docs/`
> Method: `scout` context map + 4 parallel specialist reviewers (`umans-coder` × 2, `claude-opus-4-8` × 2)
> Artifacts: `review-context.md`, `review-fat-legacy.md`, `review-unification.md`, `review-fragility.md`, `review-tests-docs.md`

## Executive summary

The codebase is well-governed on the surface: curated clippy policy, clean
TypeScript hygiene, end-to-end OpenAPI/schema drift gating, no ignored tests, and
no stray `todo!()`/`unimplemented!()`. The real leverage is in three places:

1. **Three correctness issues in the runtime/security hot path** that can degrade
   or leak under load (unbounded session mutation state, poisoned-mutex bricking,
   and never-expiring pending OAuth flows).
2. **A live documentation-drift hole**: `docs/api/endpoints.md` is committed stale
   and CI cannot catch it; plus ~165 `@spec` anchors still point at retired flat
   doc paths.
3. **Multiple unification targets** where the recent runtime↔authority split and the
   client-side replica have created parallel implementations of the same contract.

## Highest-priority findings

### Critical / Important

| ID | Theme | Finding | Location | Fix |
|---|---|---|---|---|
| F1 | Fragility | Per-session mutation state grows unbounded and is re-emitted on every reconnect | `crates/posthaste-runtime/src/sessions.rs:44-46`, `accept_mutation`, `settle_mutation`, `collapse_session_frames` | Evict settled mutations; keep only idempotency/replay window |
| F2 | Fragility | Poisoned `Mutex` errors permanently disable store writes and session registry | `crates/posthaste-store/src/store.rs:152-195`, `crates/posthaste-runtime/src/sessions.rs:~750` | Recover poison or use `parking_lot`; catch-unwind around critical sections |
| F3 | Fragility | `Pending` OAuth flows never expire → secret retention + unbounded map growth | `crates/posthaste-authority-runtime/src/oauth/flow_store.rs:84-92` | Stamp `Pending` with TTL; prune like terminal states |
| F4 | Fragility | Successful provider mutation + failed readback silently drops authoritative state | `crates/posthaste-engine/src/live_mutation.rs:48-51`, `:148-151` | Distinguish transient readback failure; retry settle for JMAP |
| T1 | Docs/tooling | `docs/api/endpoints.md` is stale and ungated | `docs/api/endpoints.md`, `tools/docs/gen_endpoints.py` | Add `gen_endpoints.py && git diff --exit-code` to CI |
| T2 | Docs/tooling | ~165 `@spec` anchors point to retired `docs/L0-*` / `docs/L1-*` paths | grep across `crates/` and `apps/web` | Bulk-rewrite to new domain-organized paths; add CI resolver |
| U1 | Unification | `posthaste-runtime` owns `AuthorityRuntime*` types despite being the near-node crate | `crates/posthaste-runtime/src/build.rs:52/316/350` | Rename to `Runtime*`; `posthaste-authority-runtime` keeps authority-side names |
| U2 | Unification | `dispatch_named_mutation` (near) and `apply_named_message_mutation` (far) duplicate the same mutation dispatch table | `runtime/src/build.rs:547`, `authority-runtime/src/backend.rs:~701` | Single message-command dispatch table in `link-contract` or `link-core` |

### Quick wins (low risk, immediate payoff)

- **Trim unused Cargo dependencies** in `posthaste-config`, `posthaste-store`, `posthaste-engine`, `posthaste-api`, `posthaste-runtime`, `posthaste-link-core`, `posthaste-server`.
- **Pin `jmap-client` to a rev** instead of a branch (`Cargo.toml:37`).
- **Switch `posthaste-runtime` from raw `tracing` macros to `ph_*!`** macros for consistent observability.
- **Add `recommendedTypeChecked` ESLint config** to catch floating/misused promises in `apps/web`.

## Theme-by-theme findings

### 1. Legacy / fat (`review-fat-legacy.md`)

- **`docs/stale/`** (24 files) is still linked from `mkdocs.yml`, `docs/index.md`, several `depends:` frontmatter entries, 5 `docs/eph/*.md` files, and multiple `@spec` anchors in Rust source. Safe to delete only after redirects are updated.
- **`AuthorityRuntimeApiMigrationBridge`** in `authority-runtime/src/build.rs:50-72` is explicit transitional debt consumed by `posthaste-server/src/migration.rs`, tests, and bench. Do not remove until `PLAN-L3-api-runtime-wrapper-migration` is complete; consider `#[deprecated]` on migration constructors.
- **Store outbox recovers first-design `conflicted` rows** (`store/src/outbox.rs:18-20`). Audit dogfood DBs for remaining rows, then remove the branch.
- **`#[allow(dead_code)]` `constant_time_eq`** in `api/src/auth/perimeter.rs:18-31` is unused by production code (Stage B caveat enforcement only).
- **15+ unused workspace/external dependencies** identified across crates. The cleanest mechanical deletion list is in `review-fat-legacy.md` §5.

### 2. Unification targets (`review-unification.md`)

- **Runtime ↔ authority symmetry:** besides the naming inversion, `LocalBackend` manually delegates ~40 `BackendApi` methods that could be generated from the same `for_each_link_op!` table used for `RemoteBackend`/`link_router`.
- **Client-side replica duplication:** the TS adapter in `apps/web/src/runtime/replica/replicaAdapter.ts` reimplements the same mutation-name→assertion mapping, diff inversion, and base+delta+pending fold that already exist in Rust `link-core`/`link-replica`. The long-term goal should be to let the WASM replica own the fold and only emit view frames.
- **Web mutation hooks:** dozens of call sites reimplement `useMutation({ mutationFn: () => runtimeMutations... })` and `useEmailActions` embeds generic dispatch plumbing. A `useRuntimeMutation` / `useRuntimeDispatch` factory would collapse ~300 LOC.
- **Outbox string-encoding duplication:** `store/src/outbox.rs` hand-rolls `parse_operation_state`/`operation_state_str`/etc. for enums already defined in `domain/src/model/outbox.rs`. Implement `Display`+`FromStr` in domain and delete store-local parsers.
- **Error code overlap:** `RuntimeErrorCode` (`runtime-contract`) and `ApiErrorCode` (`api/src/api/errors.rs`) map the same concepts through two hand-written conversion tables. Collapse into one shared code space.
- **Schema generation:** `asyncapi.json` is hand-maintained while `openapi.json` is generated. Derive `utoipa::ToSchema` for event payloads and generate AsyncAPI the same way.
- **Test fixtures:** `temp_root()` + `DatabaseStore::open(...)` is copied ~40 times. A `posthaste-test-support` crate would unify setup and teardown.

### 3. Fragility / correctness / security (`review-fragility.md`)

- **F1–F3** above are the big three.
- **Draft create/update retry can duplicate provider drafts** because `inflight` ops are re-attempted and `save_draft(None)` always creates a new provider draft.
- **cid-URL rewrite** in `api/messages/detail.rs:~95-130` splices provider-controlled `attachment.id` into already-sanitized HTML without re-escaping or re-sanitizing.
- **CSS `style` sanitization** is substring-based and bypassable via CSS escaping.
- **Startup/serve panics** on malformed CORS origin / post-bind errors (`api/src/serve.rs:95`, `:144-145`, `:174`).
- **Root key resolution silently falls through** when `POSTHASTE_MACAROON_ROOT_KEY` is set but undecodable (`api/src/token.rs:108-118`).
- **Positives:** auth perimeter, OIDC/JWT validation, smart-mailbox SQL compilation, per-account sync serialization, and store connection configuration were all found sound.

### 4. Tests / docs / tooling (`review-tests-docs.md`)

- **Highest coverage gaps:** `posthaste-engine` live network paths (`live/gateway.rs`, `live_sync.rs`, `ws_connection.rs`, `live.rs`: ~830 LOC, 0 tests), `posthaste-runtime/src/build.rs` `RuntimeCore` (1374 LOC, 0 inline tests), and `apps/mcp` (5732 LOC, no tests).
- **No property/fuzz tests** anywhere. Prime targets: `link-core` diff invertibility, `link-replica` convergence, query-language parser, sanitizer.
- **Concurrency coverage uneven:** supervisor has real race tests, but sync-trigger coalescing and runtime session-seq monotonicity are untested.
- **`asyncapi.json` message payloads** are not contract-tested; only the `EventTopic` enum is checked.
- **Superseded `docs/eph/DESIGN-L2-reversible-undo-redo.md`** still reads as forward work although the feature is shipped in `link-core` and `runtime/build.rs`.
- **`warm-release-cache.yml`** mirrors `release.yml` by hand; use a reusable workflow.
- **WASM binary freshness** is only checked at the binding-interface level, not the `.wasm` itself.
- **Vendored `imap-codec`/`imap-types`** have no `FORK.md` describing local divergence from upstream.

## Recommended action order

1. **Close the live docs drift:** gate `endpoints.md` in CI and add an `@spec` resolver check (prevents the rot from growing).
2. **Fix the three runtime/security fragility issues** (F1–F3); they are independent, bounded, and high-impact.
3. **Trim unused Cargo deps** across the identified crates (pure compile-time win).
4. **Pin `jmap-client` to a rev** (one-line reproducibility).
5. **Add tests for the hottest untested code:** `posthaste-engine` live paths and `runtime/build.rs` `RuntimeCore` (use existing mocks).
6. **Tackle unification in this order:** rename `AuthorityRuntime*` types, collapse `RuntimeErrorCode`/`ApiErrorCode`, extract shared web mutation hooks, then address the larger WASM-replica unification.
7. **Longer-term:** property tests for diff/replica invariants, reusable release workflow, AsyncAPI payload generation, vendored fork documentation.

## Artifacts

- `review-context.md` — workspace map, hotspots, dependency signals, test commands
- `review-fat-legacy.md` — dead-weight, legacy code, unused deps
- `review-unification.md` — deduplication and consolidation targets
- `review-fragility.md` — correctness, concurrency, security issues
- `review-tests-docs.md` — coverage gaps, spec drift, tooling brittleness
