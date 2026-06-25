---
scope: L1
summary: "Ephemeral revision plan for implementing the bundled application runtime in-app"
modified: 2026-06-15
reviewed: 2026-06-15
lifecycle: ephemeral
type: PLAN
depends:
  - path: docs/runtime/L1
  - path: docs/runtime/adapter/L1
  - path: docs/client/L1
  - path: docs/client/L2
  - path: docs/backend/L2
---

# Bundled application implementation revision plan

## 1. Purpose

This plan tracks the specs that still need enough detail before implementation starts in the app.

The target deployment mode is bundled application: packaged renderer plus embedded authority runtime. The UI renders runtime state. The embedded authority runtime owns SQLite, providers, views, mutations, events, config, secrets, and cache state.

The runtime contract is shared. Bundled/local-authority mode implements it with an authority runtime crate. Future hosted, multi-device, or offline modes may implement it with a local-replica runtime that owns replica state, outbox, coverage, and remote-authority sync.

## 2. Specs to revise next

### 2.1 Runtime adapter interface

Status: specified in `docs/runtime/L2.md` and `docs/client/L2.md` on 2026-06-15.

The specs now define session operations, view operations, descriptor families, snapshot/frame shapes, mutation request/settlement frames, resource operations, and adapter error shape.

Remaining implementation choices are exact Tauri command/event names and the generated/shared type source.

### 2.2 View window state

Status: specified in `docs/state/mail/L1.md`, `docs/state/mail/L2.md`, `docs/runtime/L2.md`, and `docs/client/L2.md` on 2026-06-15.

The specs now define `MailListViewState`, `MailListRowState`, continuation cursors, anchor status, lifecycle affordances, coverage/read-watermark seam, pending row markers, and UI scroll-anchor responsibilities.

Remaining implementation choices are exact TypeScript/Rust names and whether initial conversation-list rows carry full envelopes or a narrower conversation row projection.

### 2.3 Named mutation catalog

Status: specified in `docs/runtime/L2.md`, `docs/runtime/L1.md`, `docs/client/L2.md`, and `docs/backend/L2.md` on 2026-06-15.

The specs now define message keyword mutations, message mailbox/destruction mutations, draft/compose mutations, settings/account mutations, sync trigger, and support mutations for retry, undo, and conflict resolution.

Remaining implementation choices are exact Rust/TypeScript enum names, idempotency retention duration per mutation class, and provider-specific policy for labels, archive semantics, draft upload, and sent-message reconciliation.

### 2.4 Runtime contract and authority runtime extraction

Status: specified in `docs/backend/L2.md` and `docs/runtime/L2.md` on 2026-06-15.

The specs now define a shared runtime contract separate from runtime implementations, a reusable authority runtime crate for bundled/local-authority mode, transport-free handle construction, method groups, adapter-neutral caller context, API adapter state, Tauri adapter boundaries, loopback bridge containment, and graceful shutdown ownership.

Remaining implementation choices are exact Rust crate/module names, concrete trait boundaries for view/mutation/resource services, and whether `AppState` is replaced outright or wrapped around the runtime handle during migration.

### 2.5 Desktop security and storage

Status: specified in `docs/runtime/L1.md`, `docs/runtime/L2.md`, `docs/client/L1.md`, `docs/client/L2.md`, and `docs/backend/L2.md` on 2026-06-15.

The specs now define runtime-owned config/state/cache roots, renderer-owned presentation storage limits, runtime secret store ownership, client profile token separation, loopback bridge token rules, Tauri command allowlist/input validation, external navigation rules, resource capabilities, logging restrictions, and data-retention ownership.

Remaining implementation choices are exact platform path mapping, Tauri capability file contents, token retention duration for loopback bridge sessions, and cache/account deletion retention policies.

### 2.6 Tests and migration checks

Status: specified in `docs/eph/PLAN-L2-bundled-app-test-plan.md` on 2026-06-15.

The test plan defines runtime handle tests, API adapter compatibility tests, renderer adapter tests, runtime view tests, mutation coordinator tests, desktop/Tauri security tests, storage/secret tests, bundled E2E tests, migration order, fakes, and done criteria.

Remaining implementation choices are exact test filenames, mock-provider API shape, and whether some lower-level tests live as Rust module tests or integration tests.

## 3. Current implementation gap

The current desktop app starts an embedded Axum server and injects a loopback port/token into webviews. That is acceptable only as a migration bridge under the new specs.

Before implementation, create the shared runtime contract outside `posthaste-server` and extract the authority runtime handle into a reusable runtime implementation crate. Both the Axum router and the Tauri runtime adapter should call that runtime contract. Then move renderer code behind the runtime adapter facade before replacing direct HTTP-backed mail reads.
