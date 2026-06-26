# Posthaste — Test Coverage, Docs Drift & Tooling Audit

> Audit date: 2026-06-26. Scope per `review-context.md`. Evidence is file paths +
> reproductions. No files were edited. Severity: **High** (correctness/contract
> risk that can ship silently), **Medium** (real gap, partial mitigation exists),
> **Low** (hygiene / future-proofing).

---

## Executive summary

The codebase is unusually clean on the dimensions that grep-based reviews usually
catch: TypeScript hygiene is excellent (3 `any`, 1 `as any` across 40k LOC of
`apps/web`; **0** in `apps/mcp`), there are no `#[ignore]`d tests, and the
code→`openapi.json`→`schema.gen.ts` contract chain is gated end-to-end by both a
Rust test and a web check script.

The real risks are in three places:

1. **A genuine ungated schema-doc drift** — `docs/api/endpoints.md` is committed
   stale and CI cannot catch it (reproduced below).
2. **Spec traceability has rotted** — ~165 Rust files still point `@spec` at the
   pre-restructure `docs/L0-*`/`docs/L1-*` paths that the realized tree no longer
   uses. The migration claimed in memory `special-stale-migration` is incomplete.
3. **The network-facing and runtime-core hot paths are tested only indirectly** —
   the engine's live JMAP/WS gateway, `runtime/build.rs` (the `RuntimeCore`), and
   the entire MCP server have no direct tests; there are **zero** property/fuzz
   tests anywhere.

---

## 1. Test coverage gaps

### 1.1 Engine live network paths are untested — **High**
`crates/posthaste-engine/src/live/gateway.rs` (207), `live_sync.rs` (248),
`ws_connection.rs` (163), `live.rs` (212) — **0 tests each** (~830 LOC). These are
the JMAP gateway dispatch, the WebSocket connection/reconnect, and the live sync
loop: the most error-prone code in the crate (network failure, reconnection,
partial sync, correlation). The crate has **no `tests/` directory**; its inline
tests (`live_mutation/tests.rs`, `conversions/tests.rs`, etc.) cover only
request-payload shaping (e.g. "set_keywords request uses null to remove seen") —
pure happy-path translation, no failure/reconnect/timeout cases.
- **Remediation:** add `crates/posthaste-engine/tests/` exercising gateway dispatch
  and reconnection against the existing `mock.rs` double — at minimum: gateway
  rejection → error classification, WS drop → reconnect, sync error mid-batch.

### 1.2 `runtime/build.rs` `RuntimeCore` has no direct tests — **High**
`crates/posthaste-runtime/src/build.rs` (1374 LOC) implements the entire
`RuntimeCore`: `dispatch_named_mutation`, `run_apply_diff`, `run_message_mutation`,
`capture_diff`, `event_matches_filter`, scope enforcement (`ensure_account_in_scope`,
`ensure_runtime_active`). **0 inline tests.** Coverage is only transitive through
`posthaste-server` integration tests. Given this is a just-decomposed, fast-moving
file and the runtime↔authority symmetry target, the dispatch/diff-capture/scope
logic deserves unit coverage independent of a full server stand-up.
- Note: the underlying invertible-diff *algebra* **is** well covered in
  `posthaste-link-core/src/message.rs` (14 tests: inverse, idempotency, coalescing,
  replay). The gap is the runtime *wiring* of that algebra (`capture_diff` ↔
  `run_apply_diff`), and the `undoOf`/`redoOf` history navigation.
- **Remediation:** unit-test `capture_diff`→`run_apply_diff` round-trips and
  scope-rejection paths with a mock backend.

### 1.3 No property/fuzz testing anywhere — **Medium**
Zero `proptest`/`quickcheck`/`arbitrary`/`fuzz` usage across all crates (verified in
`Cargo.toml` and source). Several subsystems are textbook property-test targets and
currently rely on hand-picked examples:
- `link-core/src/message.rs`: the invertibility law `apply(apply(s,d), d.inverse()) == s`
  and coalescing-equivalence are asserted for *single* examples, not over random states.
- `link-replica` convergence (the WASM/native replica) — convergence under reordered
  assertions is exactly a property.
- `apps/web` query-language parser (`queryLanguage.test.ts`) and the API sanitizer
  (`crates/posthaste-api/src/sanitize/tests.rs`, XSS-relevant) — both example-based.
- vendored `imap-codec` parsing.
- **Remediation:** introduce `proptest` for the diff/coalesce laws and replica
  convergence first (highest correctness leverage, cheapest to express).

### 1.4 MCP server has zero tests — **Medium**
`apps/mcp` (5732 LOC) has **no `*.test.ts` files**. CI (`ci.yml` `mcp` job) only runs
`api:generate` diff + `typecheck` + `build`. The tool/handler logic (request
translation, error mapping to MCP responses) is entirely unverified at runtime.
- **Remediation:** add handler-level tests for at least the error/edge mappings;
  wire `bun test` into the `mcp` CI job.

### 1.5 Concurrency coverage is uneven — **Medium**
The supervisor is a bright spot: `supervisor/tests.rs` has real races
(`stale_runtime_generation_cannot_overwrite_current_runtime_status`,
`concurrent_progress_writes_cannot_clobber_sync_success`,
`late_sync_progress_does_not_revive_syncing_after_success`). But two adjacent
concurrency-sensitive paths are untested:
- **Sync-trigger coalescing** (`supervisor/manager.rs:158-186`, a recent hot-path
  per the churn map) — no test references `coalesc` in `supervisor/tests.rs`. The
  "trigger arrives while syncing → coalesce into single pending follow-up" invariant
  is unverified.
- **Runtime session registry / seq** (`runtime/src/sessions.rs`, 952 LOC, 3 tests) —
  all three tests cover view-diff emission (upsert/removal/structural fallback); the
  `SessionRegistry`/`RuntimeSessionSeq`/`MutationAcceptance` concurrency is untested
  at unit level.
- **Remediation:** a coalescing test (two `TriggerOnly`s during an in-flight cycle →
  exactly one follow-up) and a session-seq monotonicity test under concurrent frames.

### 1.6 `asyncapi.json` payloads are not contract-tested — **Medium**
`crates/posthaste-server/tests/asyncapi_contract.rs` checks **only** the
`components.schemas.EventTopic` *enum* against `posthaste_domain::ALL_EVENT_TOPICS`.
The event **message payload schemas** in `asyncapi.json` are hand-maintained and
nothing verifies them against the actual emitted event structs. Compare to
`openapi_contract.rs`, which round-trips the whole document. Payload drift ships
silently.
- **Remediation:** if event payloads derive `utoipa::ToSchema`, generate the
  AsyncAPI message schemas the same way `openapi.json` is generated and add a
  full-document contract test; otherwise document that payloads are best-effort.

### 1.7 Domain service hot files lean on a shared tests dir — **Low (verify)**
`domain/service/{outbox.rs(819), mutation.rs(282), message_queries.rs(421)}` have
**0 inline tests**, but outbox is covered by `service/tests/outbox.rs` (19),
`message_mutation_retries.rs`, and `message_mutation_cursors.rs`. `mutation.rs` and
`message_queries.rs` coverage is harder to trace. `outbox.rs::classify_gateway_error`
(error→`FlushError` classification) and `parse_payload` failure paths are worth a
direct check — error classification is exactly where silent regressions hide.

---

## 2. Docs / spec drift

### 2.1 `docs/api/endpoints.md` is committed-stale and ungated — **High**
Reproduced: running the generator overwrites the committed file (10 insertions, 18
deletions):
```
$ python3 tools/docs/gen_endpoints.py   # → "62 operations across 13 tags"
$ git diff --stat docs/api/endpoints.md
 docs/api/endpoints.md | 28 ++++++++++------------------
```
The committed file still lists an **`## events` / `GET /v1/events` (`stream_events`)**
section, but that path no longer exists in `openapi.json` (the vestigial SSE endpoint
was removed, commit `cce95402c`); the generator correctly drops it. Tag ordering has
also drifted.

Why CI doesn't catch it: the `docs` job in `ci.yml` (and `docs.yml`) runs
`uv run mkdocs build --strict` **directly**, not `just docs build` — so it never
regenerates `endpoints.md` and never `git diff --exit-code`s it. Contrast the
`schema.gen.ts` chain, which *is* gated in both the `mcp` job and
`apps/web/scripts/check-openapi-types.ts`. Locally, `just check` → `just docs build`
*does* regenerate it, but silently mutates the working tree with no failure, so a
stale commit slips through.
- **Remediation:** add a CI step mirroring the `schema.gen.ts` guard:
  `python3 tools/docs/gen_endpoints.py && git diff --exit-code docs/api/endpoints.md`.

### 2.2 `@spec` references mass-point at retired doc paths — **High**
~**165** Rust files carry `@spec docs/L0-*` / `@spec docs/L1-*` / `docs/stale`
anchors (e.g. `imap/src/message.rs` → `docs/L1-sync#body-lazy`,
`store/src/db/schema.rs` → `docs/L0-accounts#the-invariant`, `imap/src/mutation.rs`
→ `docs/L1-api#message-commands`). Only **33** files use the new domain-organized
paths (`@spec docs/api/`, `docs/runtime/`, …). The realized tree no longer has flat
`docs/L1-*.md` files — those names survive only under `docs/stale/` (historical-only
per the context map). So the bulk of the codebase's spec backlinks now resolve to
nothing authoritative. Web shares the rot (e.g.
`apps/web/scripts/check-openapi-types.ts` → `@spec docs/L1-api#openapi-contract`).
This contradicts memory `special-stale-migration` ("legacy @spec refs rewired").
- There is **no `@spec` validator** in `tools/`/CI, so this can only grow.
- **Remediation:** (a) add a checker that every `@spec` anchor resolves to an
  existing `docs/**` heading and fail CI on misses; (b) bulk-rewrite the `imap`/`store`
  anchors to the new tree (these two crates are the worst offenders).

### 2.3 Superseded `eph/` design still presented as forward work — **Medium**
`docs/eph/DESIGN-L2-reversible-undo-redo.md` (modified 2026-06-25) describes a
"from-scratch redesign" of undo/redo as not-yet-built, and memory
`reversible-undo-redesign` says "spec written, not built." But the feature **has
landed**: `link-core/src/message.rs` ships the invertible diff (14 tests),
`runtime/build.rs::run_apply_diff` wires it, and the **realized** docs already
describe it as done (`docs/runtime/mutations/L1.md:96` documents `message.applyDiff`;
`L2.md:120` states the undo stack is runtime-owned). The eph doc and the memory are
now historical and should be retired/marked, or they will mislead the next reader
into re-deriving a shipped design.
- **Remediation:** mark the eph design superseded (or delete) and update the memory
  note; the realized L-docs are the current source of truth.

### 2.4 `eph/` frontmatter + dependencies on retired docs — **Low**
All 18 `docs/eph/*.md` use `lifecycle: ephemeral` + `type:` but carry **no `state:`**
field, unlike the realized tree's `state: realized`. `DESIGN-L2-local-first-mutations.md`
declares `depends: docs/stale/L1-outbox` and `docs/stale/L1-sync` — i.e. an active
design hangs off retired/historical docs. Low-risk but a smell that the eph layer
hasn't been reconciled against the post-restructure tree.

---

## 3. Build / tooling brittleness

### 3.1 `endpoints.md` freshness is ungated; `just check` mutates silently — **High**
(See 2.1.) The only place `endpoints.md` regenerates is local `just check` /
`just docs build`, which overwrites in place without a diff gate. CI never runs it.
Net effect: the generated artifact's freshness depends entirely on a developer
noticing an unstaged change. This is the single weakest link in the otherwise
well-gated schema pipeline.

### 3.2 `jmap-client` pinned to a branch, not a rev/tag — **Medium**
`Cargo.toml:37`:
`jmap-client = { git = ".../jmap-client.git", branch = "feat/ws-correlation" }`.
A branch pin is non-reproducible: `cargo update` silently advances it, and the branch
can be force-pushed. `Cargo.lock` pins the exact commit *today*, but nothing prevents
drift on the next update, and a force-push can orphan the locked commit.
- **Remediation:** pin `rev = "<sha>"` (or a tag). Branch is for development only.

### 3.3 Vendored `imap-codec`/`imap-types` forks have no divergence record — **Medium**
`vendor/imap-codec` (2.0.0-alpha.7) and `vendor/imap-types` (2.0.0-alpha.6) are
`[patch.crates-io]` forks of pre-release upstream. `vendor/imap-codec/README.md` is
the upstream readme; there is no `PATCH`/`FORK` note describing *what* diverges or
*why*, so a future upstream bump is a blind merge. The forks pin alpha versions —
upstream churn risk is high.
- **Remediation:** add a short `vendor/*/FORK.md` listing the local patches and the
  upstream base commit, so the delta is auditable.

### 3.4 Committed WASM binary is never freshness-checked — **Medium**
`ci.yml` `replica-wasm` job builds with `SKIP_WASM_OPT=1` and then
`git diff --exit-code`s only the wasm-bindgen **JS/.d.ts** bindings — deliberately
not the `.wasm` (binaryen output is non-deterministic). The committed
`apps/web/src/runtime/wasm/posthaste_link_wasm_bg.wasm` is therefore only validated
at the *interface* level. A logic-only change to `link-replica`/`link-wasm` that
doesn't alter the wasm-bindgen surface won't move the bindings, so a stale `.wasm`
can ship and `replicaWasmSmoke.test.ts` will run it. This is an accepted tradeoff,
but it means "WASM is fresh" is not actually enforced.
- **Remediation:** either rebuild the `.wasm` in CI and run the smoke against the
  fresh artifact (not the committed one), or hash the wasm-bindgen *input* (the
  compiled `.wasm` pre-opt is deterministic given pinned toolchain) and gate on that.

### 3.5 `warm-release-cache.yml` duplicates `release.yml` — **Low**
Per the context map and memory `release-build-cache`, `warm-release-cache.yml`
mirrors `release.yml`'s build steps to keep the release cache warm. Two hand-synced
workflow copies are a known drift source (a build-flag change in one silently
de-warms the other). Consider extracting the shared build into a reusable workflow
(`workflow_call`) consumed by both.

---

## 4. Schema drift summary

| Artifact | Source of truth | Gated by | Status |
|---|---|---|---|
| `openapi.json` | API handlers (`utoipa`) | `server/tests/openapi_contract.rs` (CI `cargo test`) | ✅ gated |
| `apps/web/.../schema.gen.ts` | `openapi.json` | `check-openapi-types.ts` + `mcp` job diff | ✅ gated |
| `apps/mcp/src/schema.gen.ts` | `openapi.json` | `ci.yml` `mcp` job `git diff --exit-code` | ✅ gated |
| route authz table | `openapi.json` operationIds | `server/tests/authz_completeness.rs` | ✅ gated |
| `asyncapi.json` **EventTopic enum** | `ALL_EVENT_TOPICS` | `asyncapi_contract.rs` | ✅ gated |
| `asyncapi.json` **message payloads** | event structs | — | ❌ **ungated** (1.6) |
| `docs/api/endpoints.md` | `openapi.json` | — (CI runs mkdocs directly) | ❌ **stale + ungated** (2.1) |

The HTTP contract chain is solid; the two holes are the AsyncAPI payload schemas and
the endpoint inventory doc.

---

## 5. Web boundary checks — assessment

The four gating scripts are **not** trivial wiring checks and are a genuine strength:
- `check-runtime-boundaries.ts` enforces an explicit allowlist of HTTP symbols
  permitted only through `src/runtime/httpAdapter.ts` (the migration seam) — real
  architectural enforcement of the runtime-wrapper migration.
- `check-openapi-types.ts` (schema drift), `check-query-boundaries.ts`,
  `check-logging-contract.ts` similarly enforce structural invariants.

One gap, **Medium**: `apps/web/eslint.config.js` extends
`tseslint.configs.recommended` (syntactic), **not** `recommendedTypeChecked`. So the
type-aware rules are off — notably **`no-floating-promises`** and
**`no-misused-promises`**. The review brief asks specifically about "missing error
handling in async code"; with `strict: true` tsconfig but no type-checked lint,
**unhandled/floating promises are not caught**. `no-non-null-assertion` is also off
(it lives in `stylistic`), though non-null usage is currently negligible (≈5).
- **Remediation:** add `tseslint.configs.recommendedTypeChecked` (or at least enable
  `no-floating-promises` + `no-misused-promises`) to gate async error handling.

---

## 6. TypeScript hygiene — assessment

This is a **strength**, not a gap. `apps/web`: 3 `: any`, 1 `as any`, ~5 `as unknown`
across 40k LOC, `strict: true`. `apps/mcp`: 0 of each, `strict: true`. The only
actionable items are the lint-config gap (§5) and MCP's absent tests (§1.4). No
remediation needed on cast/`any` discipline.

---

## Recommended priority order

1. **Gate `endpoints.md`** in CI (§2.1/§3.1) — one CI line; closes a live drift today.
2. **Add an `@spec` resolver check + rewrite `imap`/`store` anchors** (§2.2) — restores
   spec traceability; otherwise it only worsens.
3. **Pin `jmap-client` to a rev** (§3.2) — one-line reproducibility fix.
4. **Engine live-path + `RuntimeCore` tests** (§1.1/§1.2) — highest correctness leverage
   on the fastest-moving, network-facing code.
5. **Retire the superseded reversible-undo eph design + fix the memory** (§2.3).
6. **`recommendedTypeChecked` lint** (§5) and **first `proptest`s** (§1.3) — durable
   guardrails for async errors and the diff/replica invariants.
