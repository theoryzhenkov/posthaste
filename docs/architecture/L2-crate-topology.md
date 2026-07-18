---
title: "Crate topology (L2)"
scope: L2
summary: "Crate topology — the one place the workspace's crate set, ownership, dependency direction, and role binaries are named. Describes the integrated-app workspace: the core library crates under crates/, the app crates under apps/client, and the one documented layering exception (replica-core via domain-service)."
modified: 2026-07-18
reviewed: 2026-07-18
state: active
depends:
dependents:
---

# Crate topology

This spec names the workspace boundary once: which crates exist, what each
owns, the dependency direction, and the role binaries. The split-model crate
set (server/runtime/link/wasm tiers) was retired with the pivot to the
integrated app; its final pre-deletion tree is preserved on the
`legacy/split-model-final` branch.

Naming rules: a crate is named by **what it owns**. The integrated app's
components carry the plain component names — **backend** (the Rust process
that owns all state and evaluates every query), **frontend** (the TypeScript
mirror), **desktop** (the Tauri shell) — and app crates are prefixed
`posthaste-client-*` after the product they assemble. Bin names are
hyphenated, never underscored. A name that overclaims is treated as a bug.

## 1. The crate set

Seventeen crates: fourteen libraries/tools under `crates/`, three app crates
under `apps/client/` (the frontend is TypeScript, not a crate).

### 1.1 Core tier (pure vocabulary and services)

| Crate | Owns | May depend on |
|---|---|---|
| `posthaste-domain-model` | The pure domain types: ids, messages, records, commands, outbox/sync/rev-log types, smart mailboxes, account settings/overview, appearance, automation, notifications, vocab (`MailboxRole`, `SystemKeyword`), typed errors, plus the pure cache/imap/provider slices the model types' inherent impls close over. | — |
| `posthaste-replica-core` | The effect-fold leaf: the convergence fold (`Replica`, `MessageReplica`, `MutationId`, settlement fold) that computes `visible = fold(base, intent_log)`. *Below* domain; domain-free by construction. **The one surviving split-model crate**: `domain-service` consumes its fold as the single fold engine for the NS1 overlay (RFC-L2-client-replication-model §6 — SQL is the single predicate engine, replica-core the single fold). | — |
| `posthaste-observability` | Telemetry: tracing/log setup and instrumentation helpers. | — |
| `posthaste-query-grammar` | The one query grammar: the tokenizer + `parse_query`/`parse_query_with_scopes`/`ScopeToken` that compile human-readable search strings into `SmartMailboxRule` trees, shared by smart mailboxes and the rules engine's WHEN-clause grammar. | domain-model |
| `posthaste-call-policy` | The outbound-call **policy core**: the pure arithmetic that decides *how* an outbound provider call retries, backs off, deadlines, and classifies — `BackoffSchedule`, the per-`CallClass` `DeadlinePolicy` table, `classify_status`/`resolve_terminality` over the shared `Terminality` taxonomy (owned by domain-model, consumed here, never re-minted). Clock/RNG-free — every entry point takes explicit `now`/`attempt`/`rand_unit` inputs. | domain-model |
| `posthaste-domain-service` | The hexagonal core: `MailService`, all port traits (`MailGateway`, `MailStore` composite, secret/config/push ports), the typed-intent outbox and overlay fold (NS1/NS2), imap planning + identities logic, cache scoring/governor, `validate_*` functions, the `BaseWrite` capability seal. | domain-model, replica-core, call-policy, observability |

### 1.2 Adapter tier (native)

| Crate | Owns | May depend on |
|---|---|---|
| `posthaste-store` | SQLite adapter (`DatabaseStore`, `RepairReport`): base tables (provider truth, sync-only writes), the `message_overlay` table, the `_effective` views every read goes through. Exports only what it owns — no re-exports of domain symbols. | domain-service (+model), observability |
| `posthaste-engine` | JMAP gateway/push adapters. Routes its outbound JMAP calls through `provider-call`. | domain-service, provider-call, observability |
| `posthaste-provider-call` | The native outbound-call envelope: the tokio/reqwest *executor* half over the `call-policy` core — the shared `reqwest::Client` pool, per-class deadlines (total timeout or between-chunks stall guard for blobs), the `Retry-After`-aware jittered retry loop, the per-account circuit breaker (`ProviderCallExecutor`). Only `engine` depends on it. | call-policy, domain-model |
| `posthaste-imap` | IMAP gateway adapter. | domain-service, call-policy, observability |
| `posthaste-config` | TOML config persistence (`TomlConfigRepository`, tuning schemas). | domain-service (types via domain-model) |

### 1.3 App tier (`apps/client`)

The integrated app is one product in four parts, three of them crates:

| Crate / part | Owns | May depend on |
|---|---|---|
| `posthaste-client-models` (`apps/client/models`) | The typed localhost API vocabulary shared with the frontend: query/command/event shapes. The `export-ts` bin generates the TypeScript types the frontend imports — one schema pipeline, models as the source. | domain-model |
| `posthaste-client-backend` (`apps/client/backend`) | **The backend**: assembles the domain service over the SQLite store with the JMAP/IMAP gateways and config, and serves the localhost HTTP + SSE integration surface (queries, commands, the event stream). Runnable standalone for dev and headless use. | client-models, domain-service, store, engine, imap, config, domain-model, observability |
| `posthaste-client-desktop` (`apps/client/desktop`) | **The desktop shell**: the Tauri app that embeds the backend in-process and hosts the frontend webview; updater/notification plumbing. Intentionally thin — client-backend, observability, and the tauri crates, nothing else. | client-backend, observability |
| `apps/client/frontend` (TypeScript, not a crate) | **The frontend**: the React mirror — declares live queries through the client facade, renders results, posts commands; refetches on the SSE generation stream. Runs in the Tauri webview or a plain browser tab. | (talks to the backend over localhost only) |

### 1.4 Tooling tier

| Crate | Owns | May depend on |
|---|---|---|
| `posthaste-testkit` | Dev-only shared test support (`[dev-dependencies]` only): the disposable `Harness`, declarative TOML fixtures, `StalwartFixture`, `GmailImapFixture` (+ the `mock-gmail` dev server bin), path/port helpers. Contract: `docs/testing/L1.md`. | domain-service, store, imap, config, domain-model |
| `posthaste-bench` | Benchmarks and the `posthaste-profile` bin. | domain-service, store, query-grammar, domain-model |
| `posthaste-lab` | The lab daemon/tooling for dev stacks. | config |

### 1.5 Role binaries

A binary is named after the component it runs — no more, no less:

| Binary | Runs | Ships from |
|---|---|---|
| `posthaste-client` | The desktop app: Tauri shell + embedded backend + frontend. The nightly artifact. | apps/client/desktop |
| `posthaste-client-backend` | The backend, standalone (dev / headless / browser-tab frontend). | apps/client/backend |

Tool bins (`posthaste-lab`, `posthaste-profile`, `export-ts`, `mock-gmail`)
keep their names. Bin names are hyphenated, never underscored.

## 2. Dependency direction

```
replica-core ─┐
domain-model ─┼─► domain-service ─► store / engine / imap / config   (adapters)
observability ┘         ▲                     │
      │                 │ (engine ─► provider-call ─► call-policy)
      ├─► query-grammar ┤                     │
      ├─► call-policy ──┘                     ▼
      └─► client-models ──► client-backend (apps/client) ─► client-desktop
```

Rules:

- **No upward edges.** A vocabulary crate never depends on a service crate; a
  service crate never depends on an adapter; nothing depends on an app crate.
  App crates compose adapters; adapters never depend on each other (the one
  routed edge is `engine → provider-call`, the outbound-call executor).
- **The effect fold lives below domain.** `replica-core` stays the narrow
  domain-free leaf; the `domain → replica-core → domain` cycle is avoided by
  the minimal fold-state predictor. This is the **one documented exception**
  to "split-model crates are gone": `domain-service` owns the overlay fold
  and replica-core is its single fold engine — it survives on that edge, not
  as a client-replica seam.
- **One schema pipeline.** The frontend's types are generated from
  `client-models` (`export-ts`); no second codegen path exists.
- **Testkit is dev-only.** `posthaste-testkit` appears only under
  `[dev-dependencies]`; production crates never depend on it.

## 3. Assertions

| id | assertion |
|---|---|
| `crate-named-by-ownership` | Every crate name states what the crate owns; app crates carry the `posthaste-client-*` product prefix. |
| `binary-named-by-component` | Every role binary is named exactly after the component it runs (§1.5); hyphenated. |
| `no-upward-deps` | The dependency graph respects §2; adding an upward edge is a spec violation, not a Cargo.toml detail. |
| `replica-core-single-consumer` | `replica-core` is consumed only via `domain-service` (the overlay fold); no new consumer revives it as a replication seam. |
| `no-parallel-namespaces` | No crate re-exports another crate's public surface; the frontend's types come from the one `client-models` pipeline. |
| `testkit-dev-only` | `posthaste-testkit` is referenced only from `[dev-dependencies]`. |
