# LOC-Reduction Audit — Synthesis

> Goal: shrink what an AI agent must load to understand Posthaste.
> Method: 8 parallel scouts over the whole tree (crates + apps + docs + artifacts), 2026-06-29.
> Per-partition detail in `loc-reduction/01..08-*.md`. Baseline ≈187k LOC source + ~47k generated/vendored.

## The headline

The biggest context wins are **not** source deletions — they're **excluding machine output
and stray copies** from what the agent reads. ~47k lines (≈25% of everything an agent loads)
come off the table with near-zero risk and no behavior change. Real hand-written code is
already fairly lean; ~3.2k LOC of genuine source reduction exists on top.

| Tier | What | Lines | Risk | Behavior Δ |
|---|---|---|---|---|
| **0 — generated** | context-exclude generated artifacts | **~19,200** | ~none | none (stay in git) |
| **0b — vendor** | drop the unused `vendor/` fork copy | **~28,200** | ~none | none (build uses pinned git rev) |
| **1 — stale docs** | delete/archive scratch + shipped-design docs | **~7,250** | low | none |
| **2 — real source** | macro-collapse + dead-code + test de-bloat | **~3,200** | low–med | none |
| **3 — stretch** | design-gated refactors (not mechanical) | ~1,300+ | med–high | none |

Tiers 0+0b+1 ≈ **54.6k context-lines** for essentially mechanical, low-risk work.

---

## Tier 0 — context-exclude generated files (~19.2k lines, one config change)

All confirmed generated (generator traced) and committed on purpose (downstream builds need
them without a Rust/wasm toolchain). Do **not** gitignore — mark them so agents/diffs skip them.

| File | Lines | Generator |
|---|---|---|
| `openapi.json` | 7,715 | utoipa, `UPDATE_OPENAPI=1 cargo test` (drift-gated) |
| `apps/mcp/src/schema.gen.ts` | 5,199 | `openapi-typescript` |
| `apps/web/src/api/schema.gen.ts` | 5,181 | `openapi-typescript` |
| `apps/web/src/runtime/wasm/*.js`,`*.d.ts` | ~785 | wasm-bindgen (`just build-replica-wasm`) |
| `docs/api/endpoints.md` | 158 | `tools/docs/gen_endpoints.py` |
| `asyncapi.json` | 150 | drift-checked vs `ALL_EVENT_TOPICS` |

**Action:** add `linguist-generated=true` entries to `.gitattributes` for these, and mirror
the list in whatever context-ignore the harness honors (`.aiexclude`/equivalent). Single
lowest-risk, highest-leverage change in the whole audit.

## Tier 0b — drop the unused `vendor/` fork (~28.2k lines)

`vendor/imap-codec` + `vendor/imap-types` = 28,167 LOC / 1.2 MB. **Verified unused:**
`git ls-files vendor/` → 0 (untracked); `Cargo.toml` excludes it; `Cargo.lock` resolves
both crates from `git+https://github.com/theoryzhenkov/imap-codec.git?rev=2d19dd17…`.
It is a stray local working copy of a fork that is already pinned by rev on GitHub.

**Action:** remove `vendor/` from the workspace (zero build impact). If a local copy is
wanted for audit/offline, keep it but add a `vendor/*/FORK.md` divergence note — and still
exclude it from context.

## Tier 1 — stale docs & scratch (~7,250 lines, delete/archive)

- **Root scratch (2,797):** the 9 `review-*.md` (2026-06-26 audit output) + `context.md`
  (landed runtime-decomp brief) + `design-wasm-replica.md` (landed). Clutters the
  high-signal repo root an agent reads first. *(These `loc-reduction/*.md` files join this
  bucket once acted on.)*
- **`docs/eph/` (25 files, ~4,454):** not in mkdocs nav; many describe shipped work
  superseded by durable specs (undo-redo revlog/synced-history, mutation-notification,
  render-flicker-tracker, public-beta-readiness). **Triage** — archive completed, keep
  active plans. Conservative deletable ≈3,500.
- **`docs/issues/` (12 files, ~1,250):** working tracker, not published; some resolved per
  project memory. Triage, ≈600 safely closable.

## Tier 2 — real source LOC reductions (~3,200 lines)

The recurring win is **collapsing hand-written op pass-through tables onto the existing
`for_each_link_op!` x-macro** (the codebase already uses it to generate `RemoteBackend` and
the request structs — this just extends it):

| ID | Where | Lines | Risk | How |
|---|---|---|---|---|
| links-F1 | `link-contract/lib.rs` `BackendApi`+`BackendLink` triple-listed | ~430 | med | emit trait defaults + delegations from `for_each_link_op!` |
| rt-DUP2 | `runtime/build.rs` `RuntimeHandle: RuntimeCore` (51× `ensure_runtime_active`) | ~270 | med | declarative `runtime_delegate!` table |
| rt-DUP1 | `authority-runtime/local_backend.rs` 35 hand-delegations | ~250 | med | same macro `RemoteBackend` already uses |
| web-F4/F5 | `runtime/adapter.ts` Proxy + `httpAdapter.ts` shorthand | ~115 | low–med | Proxy stub + ES shorthand for 1:1 delegations |

**Dead code (pure deletion):** testkit `replica_probe.rs` (122) + `open_capture`/`FrameCapture`
(70); web legacy view-subscription path (150); imap dead fetch wrappers (105) +
`examine_imap_mailbox` (25); api `constant_time_eq` (30); 12 unused `LogEvent` consts (18);
web dead fns `currentSearchableServerQuery`/`getNotificationsSnapshot` (13). ≈**530 LOC**.

**Boilerplate → macro/derive:** `SyncBatch` literals `..Default::default()` (150); config
`bimap_enum!` for 1:1 Toml↔domain enums (120); store outbox enum⇄string via `Display`/`FromStr`
(45); `encode/decode_payload` helpers (45); `RuntimeError` + `ph_*!` ctor macros (65). ≈**425 LOC**.

**Test de-bloat (shared fixtures, no coverage loss):** domain service-test fixtures (120);
`TestRuntime` fixture in 2,647-line handle test (180); `posthaste-test-support` for
`temp_root`/`TestSecretStore` copied ~40× (150 in-scope). ≈**450 LOC**.

## Phase C outcome (delegation-macro collapse) — partially done, with a finding

Only delegation impls with **uniform bodies driven by the shared `for_each_link_op!`
table** are clean macro-collapse wins:

- **`RemoteBackend`** (runtime) — already generated (pre-existing).
- **`BackendLink`** (link-contract) — **DONE, −219 LOC.** Inherent async methods, no
  `async_trait`; `backend_link_delegations!` emits one `self.transport.$m($args).await`
  per row. (Additively exposes the ~13 table reads it lacked; pure delegations.)

**`LocalBackend` and `RuntimeHandle` are NOT clean targets** — they delegate to
objects with **non-uniform signatures**, so one emitter can't generate them:
- `LocalBackend`→`Backend`: ~half sync (no `.await`), several by-reference args,
  `destroy_message`→`destroy` rename, `account_count`→`Ok(...)` wrap.
- `RuntimeHandle`→`RuntimeCore`: dual `reads`/`backend_link` routing, dropped
  `_caller`, ~8 bespoke methods, `#[async_trait]` (must emit the whole impl).

A bespoke directive macro for either re-encodes each signature anyway (real saving
~3 lines/method) and hides per-method behavior — fragile for the gain. Recommend
leaving both as explicit code unless `Backend`/`RuntimeCore` are first made
signature-uniform (a separate, larger refactor).

## Tier 3 — stretch (design-gated, not mechanical; ~1,300+)

- Co-generate the `RuntimeCore` trait signatures from the op table too (rt-DUP3, ~230, high).
- Collapse the TS three-layer mutation plumbing (`api/client`→`httpAdapter`→`runtimeMutations`)
  — a "pick two of three layers" design call, not a trim.
- Route-descriptor macro for the 4× restated route fan-out in `api/` (high; authz map is a
  security artifact — leave).
- **Relocate** `runtime/fakeAdapter*` (872 LOC test scaffolding) out of `src/` → `test/` —
  0 net LOC but shrinks the `runtime/` tree an agent loads as "the client."

---

## Myths the scouts corrected (don't chase these)

- **"useRuntimeMutation factory saves ~300 LOC"** — only 11 `useMutation` sites, each with a
  genuinely distinct `onSuccess`; realistic saving ~25 LOC. Web components are already
  well-factored (no file >400 LOC, ~0 dead).
- **"TS `replicaAdapter.ts` reimplements the Rust fold (730 LOC)"** — that file is gone; the
  fold/diff/inversion already live in WASM. The remaining `entityStoreAdapter.ts` is genuine
  orchestration whose dense comments *are* the context (shipped-bug provenance). Don't trim.
- **U1/U2 unification targets** from the 2026-06-26 review are **already implemented** here.
- **`docs/stale/`** is already deleted.

## Recommended order

1. **Tier 0 + 0b** (one `.gitattributes`/context-ignore change + remove `vendor/`): ~47k
   context-lines, minutes of work, ~zero risk.
2. **Tier 1** doc triage: ~7k lines; confirm-then-delete (some eph/issues still active).
3. **Tier 2 macro collapses** in order links-F1 → rt-DUP1/DUP2 (same mechanism, ~950 LOC),
   then dead-code deletions, then boilerplate macros, then test fixtures.
4. **Tier 3** only with explicit appetite — these add abstraction or touch contract surfaces.
