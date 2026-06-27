# Posthaste — Legacy / Dead-Weight / Redundant Code Audit

Audit scope: Rust crates in `crates/`, the `docs/stale/` tree, and selected Cargo
 dependency lists. Evidence is from the codebase at `/home/usr.prj_posthaste/src`
 on 2026-06-26.

## Correct / no action needed

- **No `todo!()` or `unimplemented!()` in any crate.** The only in-code TODOs are
  two `TODO(S3)` comments in `posthaste-domain/src/service/sync_ops.rs:30` and
  `:395`.
- **`openapi` is the only feature flag**, and it is intentionally used in
  `posthaste-domain` and `posthaste-runtime-contract` for `utoipa::ToSchema`.
  No leftover `cfg(feature)` control flow exists.
- **`posthaste-observability` is not dead.** It is referenced by `ph_*!` macros
  and `events::*` constants across `posthaste-domain`, `posthaste-engine`,
  `posthaste-api`, `posthaste-authority-runtime`, `posthaste-server`, and
  `apps/desktop/src/lib.rs`.
- **`posthaste-link-wasm` is not dead.** It is built by `justfile:70-78`, imported
  by `apps/web/src/runtime/replica/handle.ts:95`, and documented in
  `docs/replication/client-link/L2.md` and `L3.md`.
- **`posthaste-bench` is actively used.** It is exercised by
  `.github/workflows/profile.yml` and `tools/lab/suites.toml:149-165`.
- **`posthaste-lab` is actively used.** The binary is invoked by
  `tools/lab/justfile` and the root `justfile:39` (`config validate`).

## Actionable findings

### 1. `docs/stale/*` cannot be deleted without first updating references

**Files / paths:**

- `docs/stale/` contains 24 legacy markdown files (flat `L0-*` / `L1-*` naming).
- Still referenced by:
  - `mkdocs.yml:98-122` — full "Stale specs" navigation section.
  - `docs/index.md:19` — links to `stale/L0-api.md`.
  - `docs/api/L1.md:19-26` and `docs/backend/L1.md:34-37` — `depends:` frontmatter.
  - `docs/eph/*.md` (5 eph docs) — `depends:` and narrative references.
  - Rust source via `@spec docs/stale/...` anchors:
    - `crates/posthastore/src/mutations/sync_batch.rs:220/256/294`
    - `crates/posthaste-domain/src/ports/gateway.rs:14/41`
    - `crates/posthaste-domain/src/ports/sync_store.rs:116`
    - `crates/posthaste-domain/src/service/sync_ops.rs:8/127/176`
    - `crates/posthaste-domain/src/model/sync.rs:41/61`
    - `crates/posthaste-authority-runtime/src/account_repository.rs:559`
    - `crates/posthaste-engine/src/sync/email.rs:46/63/198`
    - `crates/posthaste-engine/src/live_sync.rs:104`
    - `crates/posthaste-engine/src/live/gateway.rs:34`

**What to do:**

1. Replace the `@spec docs/stale/L1-sync#progressive-delivery-and-final-reconciliation`
   anchors with the equivalent new `docs/replication/` or `docs/state/mail/` anchors.
2. Update `depends:`/`reviewed` metadata and `mkdocs.yml` to point at current specs.
3. After no file outside `docs/stale/` references the tree, either delete the
   directory or move it to `docs/archive/`.

**Risk / confidence:** High confidence this is safe *after* the redirect work;
 deleting now would break nav, code-level spec links, and frontmatter
 dependencies. **Estimate: medium effort.**

---

### 2. Temporary API-runtime migration bridge is explicit legacy code

**File:** `crates/posthaste-authority-runtime/src/build.rs:50-72`

**What:** `AuthorityRuntimeApiMigrationBridge` and the `from_api_bridge_*_for_migration`
 constructors are labeled with:

```text
spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#legacy-fields-temporary
```

The struct exposes direct `service: Arc<MailService>`, `store: Arc<dyn MailStore>`,
`secret_store`, and `event_sender` so that API/app-state harnesses can still reach
through the runtime graph.

**Evidence of use:**

- Consumed by `posthaste-server/src/migration.rs`.
- Heavily used in `crates/posthaste-authority-runtime/tests/authority_runtime_handle.rs`
  and `crates/posthaste-bench/src/runtime_workload.rs`.
- `posthaste-server/tests/runtime_wrapper.rs:640-659` explicitly asserts that
  `AuthorityRuntimeCore` should *not* depend on `api_bridge`.

**What to do:** Do **not** remove yet. When
`PLAN-L3-api-runtime-wrapper-migration` is complete, remove
`AuthorityRuntimeApiMigrationBridge`, `api_bridge`, and the migration
constructors, and fold any remaining callers through the runtime handle.
Consider adding `#[deprecated]` to the public migration constructors now so new
uses stop.

**Risk / confidence:** This is known, tracked debt. Removing it today breaks
server migration and a large test suite. **Risk: high until migration lands.**

---

### 3. Store outbox still recovers first-design dogfood rows

**File:** `crates/posthaste-store/src/outbox.rs:18-20`

```rust
// Legacy dogfood rows from the first outbox design parked forever as
// `conflicted`; recover them into the new retryable state ...
"conflicted" => Ok(OperationState::Pending),
```

**What to do:** Audit production/dogfood databases for any remaining
`state = 'conflicted'` rows. Once they are gone (or after a one-time migration),
delete this branch and the comment.

**Risk / confidence:** Low risk after row audit. If any real `conflicted` rows
remain, removing the branch will make them raise `StoreError::Failure` instead
of draining.

---

### 4. `#[allow(dead_code)]` helper is only used by tests / re-export

**File:** `crates/posthaste-api/src/auth/perimeter.rs:18-31`

```rust
#[allow(dead_code)]
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool { ... }
```

**What:** The function is re-exported in `auth.rs:30` but is not consumed by
production code. Comment says it is retained for "Stage B caveat enforcement."

**What to do:** Either use it in the caveat-equality path, or remove it and its
unit test once no consumer needs constant-time comparison.

**Risk / confidence:** Not a blocker; small dead-weight item.

---

### 5. Unused Cargo dependencies  — **RESOLVED 2026-06-27** (`51b559a5`)

Removed the grep-verified-unused deps across posthaste-config / -store / -engine /
-authority-runtime / -runtime / -api / -link-core / -link-contract (20 dep lines).
`reqwest` kept in -authority-runtime (used via the `oauth2::reqwest` re-export);
`tokio` kept where `#[tokio::test]` needs it. Verified by grepping every usage form
incl. derive/macro (`#[derive(Serialize/Error)]`, `#[async_trait]`, `stream!`, `json!`,
`Uuid`, `#[tokio::]`, `time::OffsetDateTime`) — no `thiserror` derive exists anywhere;
`cargo check --workspace --all-targets` green. **NOT done here:** the
`posthaste-server` subset (§5.7) + the path-crate → `[dev-dependencies]` moves
(riskier; separate pass). Separately, the vestigial `runtimeObjectViewsEnabled()`
TS flag was retired (`59b07bc5`) — the last behavior flag of the reactive-store
migration.

Evidence generated by `cargo check --workspace --all-targets` with
`-Wunused_crate_dependencies`, then verified by grepping each package's source
for direct use. For each crate I list the dependency lines to remove (or move to
`[dev-dependencies]` when only integration tests use them).

#### 5.1 `posthaste-config`

**File:** `crates/posthaste-config/Cargo.toml`

**Remove from `[dependencies]`:**

- `serde_json` (line 9)
- `thiserror` (line 10)
- `time` (line 11)
- `tokio` (line 15) — if tests use `#[tokio::test]`, keep/migrate to `[dev-dependencies]`.

**Why:** No `serde_json::`, `thiserror::`, `time::OffsetDateTime`, or `tokio::`
imports in `crates/posthaste-config/src/`.

**Risk:** Low. `posthaste-config` depends on `posthaste-domain` which brings in
`serde`/`time` transitively, so proc-macro expansion won't break.

#### 5.2 `posthaste-store`

**File:** `crates/posthaste-store/Cargo.toml`

**Remove from `[dependencies]`:**

- `async-trait` (line 7)
- `serde` (line 12)
- `thiserror` (line 15)
- `time` (line 16)
- `tokio` (line 17)

**Why:** No direct imports in `src/`. The crate uses `std::time` and
`serde_json::` but not `serde::`/`time::`/etc.

**Note:** `serde_json` (line 13) *is* used. `tokio` may be needed only by
`#[tokio::test]` integration tests; if so move it to `[dev-dependencies]`.

**Risk:** Low.

#### 5.3 `posthaste-engine`

**File:** `crates/posthaste-engine/Cargo.toml`

**Remove from `[dependencies]`:**

- `thiserror` (line 17)

**Why:** No `#[derive(Error)]` or `thiserror::Error` usage in the crate.

**Risk:** Low.

#### 5.4 `posthaste-authority-runtime`

**File:** `crates/posthaste-authority-runtime/Cargo.toml`

**Remove from `[dependencies]`:**

- `posthaste-link-replica` (line 23)
- `thiserror` (line 31)

**Also consider removing:**

- `reqwest` (line 28) — the crate only accesses reqwest via `oauth2::reqwest`
  re-exports (`oauth/service.rs:21-22`). `oauth2` is still required.

**Why:** No direct `use posthaste_link_replica::`, `thiserror::`, or
`reqwest::` imports in `src/`.

**Risk:** Low for `posthaste-link-replica`/`thiserror`. Medium-low for `reqwest`;
verify no internal module starts importing `reqwest` directly after removal.

#### 5.5 `posthaste-runtime`

**File:** `crates/posthaste-runtime/Cargo.toml`

**Remove from `[dependencies]`:**

- `uuid` (line 31)
- `tokio-stream` (line 36)

**Why:** No `uuid::` or `tokio_stream::` usage in `src/`.

**Risk:** Low.

#### 5.6 `posthaste-api`

**File:** `crates/posthaste-api/Cargo.toml`

**Remove from `[dependencies]`:**

- `async-stream` (line 19)
- `async-trait` (line 20)
- `futures-util` (line 24)
- `jsonwebtoken` (line 25)
- `thiserror` (line 35)

**Why:** No direct imports in `src/`. The token/auth code uses `macaroon`, not
`jsonwebtoken`. No `#[async_trait]` or `#[derive(Error)]` usages.

**Risk:** Low.

#### 5.7 `posthaste-server`

**File:** `crates/posthaste-server/Cargo.toml`

**Remove from `[dependencies]` (no direct use in `src/` or binaries):**

- `ammonia` (line 24)
- `async-stream` (line 25)
- `async-trait` (line 26)
- `base64` (line 28)
- `dirs` (line 29)
- `jsonwebtoken` (line 32)
- `keyring` (line 33)
- `macaroon` (line 34)
- `oauth2` (line 35)
- `thiserror` (line 48)
- `toml` (line 50)
- `tracing-subscriber` (line 56)
- `url` (line 57)
- `utoipa-swagger-ui` (line 59)
- `uuid` (line 60)

**Move to `[dev-dependencies]` (used only by integration tests, not by the
 library or binaries):**

- `posthaste-engine` (line 40)
- `posthaste-imap` (line 41)
- `posthaste-store` (line 45)
- `posthaste-link-core` is already in `[dev-dependencies]` (line 66), which is
  correct.

**Why:** `posthaste-server/src/lib.rs` re-exports the near platform from
`posthaste_api` and composes the backend through `posthaste_authority_runtime`. It
does not directly use the listed crates. Integration tests (e.g.
`tests/provider_parity`, `tests/stalwart_identity_transport`,
`tests/backend_link_wire`) are the only consumers of the path crates above.

**Risk:** Medium for the path-crate moves because many integration tests import
`posthaste_imap`, `posthaste_engine`, `posthaste_store`, and
`posthaste_link_core`. The external-crate removals are low risk. The `tower`
entry at line 68 is already in `[dev-dependencies]` and should stay there.

#### 5.8 `posthaste-link-core`

**File:** `crates/posthaste-link-core/Cargo.toml`

**Remove from `[dependencies]`:**

- `serde_json` (line 18)

**Why:** No `serde_json::` or `serde_json` macro usage in `src/`; only `serde`
is used.

**Risk:** Low.

#### 5.9 `posthaste-link-contract`

**File:** `crates/posthaste-link-contract/Cargo.toml`

**Remove from `[dev-dependencies]`:**

- `tokio-stream` (line 30)

**Why:** It is declared under `[dev-dependencies]` and not used in tests.

**Risk:** Low.

---

### 6. `posthaste-bench` dependency hygiene

**File:** `crates/posthaste-bench/Cargo.toml`

`criterion` and `iai-callgrind` are declared in `[dev-dependencies]` but only the
criterion/iai bench targets (`benches/store_criterion.rs`, `benches/store_iai.rs`)
use them. The lib/binary targets do not, which is why `cargo` reports them as
unused for those targets. This is structurally correct; no deletion needed.

If you want to silence the warnings or shrink compile units, declare them under
`[[bench]]` target-specific dependencies (requires Cargo 1.60+ per-bench
`[bench-dependencies]` syntax) or accept the warnings because they are
intentionally scoped to the bench harnesses.

---

### 7. Stale `@spec` anchors using old flat doc names

**Observation:** There are roughly **600** `@spec docs/L1-*` or
`@spec docs/L0-*` anchors in Rust source that pre-date the domain-organized spec
restructure. They are not under `docs/stale/` but point to flat names that now
only exist as redirects or aliases. Examples (`grep -R '@spec docs/L1-' src/`):

- `crates/posthaste-api/src/api/settings.rs:5` — `@spec docs/L1-api#settings`
- `crates/posthaste-config/src/lib.rs:3` — `@spec docs/L1-accounts#config-directory-layout`
- `crates/posthaste-domain/src/service/outbox.rs` — `@spec docs/L1-outbox#operation-model`

**What to do:** Bulk-update these to `docs/<domain>/L<N>.md` anchors as part of
the same pass that removes `docs/stale/`. They are comment-only but are the
project's SPECial traceability links.

**Risk:** Very low (comments only), but broad surface.

---

### 8. Integration test suites import the full server dependency graph

**Files:** `crates/posthaste-server/tests/*`

**Observation:** Many integration tests (`backend_link_split`,
`stalwart_identity_transport`, `provider_parity`, `runtime_wrapper`, etc.) import
`posthaste_engine`, `posthaste_imap`, `posthaste_store`, and `posthaste_link_core`
directly. This is why those crates are still needed in the package. Once the
server's public surface is fully wrapper-driven, the tests can be refactored to
use only `posthaste_server::` and `posthaste_authority_runtime::` types, allowing
the lower-level crates to drop out of `posthaste-server` entirely.

**Risk:** Tied to finding #2 (migration bridge); medium effort.

---

## Summary priority

1. **High impact, low risk:** trim unused Cargo deps in
   `posthaste-config`, `posthaste-store`, `posthaste-engine`, `posthaste-api`,
   `posthaste-runtime`, `posthaste-link-core`, and the external deps in
   `posthaste-server`. This is purely mechanical and reduces workspace compile
   units.
2. **Medium impact, medium risk:** reorganize `posthaste-server` dependencies
   (move path crates to `[dev-dependencies]`) and clean up the stale doc
   references. Requires test-run verification.
3. **Long-term architectural:** retire `AuthorityRuntimeApiMigrationBridge` once
   `PLAN-L3-api-runtime-wrapper-migration` is complete, and remove the
   `conflicted` outbox compatibility branch once dogfood data is clean.
