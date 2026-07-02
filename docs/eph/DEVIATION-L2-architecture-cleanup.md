---
scope: L2
summary: "DEVIATION register — reality ledger for the architecture-cleanup refactor. One row per drained divergence between a durable spec's end-state and the code as it stands. Rows open when a decision drains from the RFC into a spec; rows close when the code lands and the spec's [::state] marker flips. Companion: RFC-L2-architecture-cleanup (decision log / drain)."
modified: 2026-07-02
reviewed: 2026-07-02
lifecycle: ephemeral
type: DEVIATION
state: planned
depends:
  - path: eph/RFC-L2-architecture-cleanup
dependents: []
---

# DEVIATION — Architecture Cleanup (reality ledger)

This register carries **reality**: for every drained RFC decision, one row
describing how the code *currently* diverges from the spec's end-state. Specs
carry intent; this file carries the delta; reality is never edited back into a
spec.

Row lifecycle: `open` (spec updated, code lags) → `landing` (migration step in
flight) → `closed` (code matches spec; `[::state]` marker removed; `reviewed`
bumped).

## Rows

| Row | RFC ref | Spec section | Current state (reality) | End state (intent) | Status |
|-----|---------|--------------|--------------------------|--------------------|--------|
| V1 | D1, D3, D4, D17, D18 | replication/authority-server-link L1–L3; authority-server L1–L3; architecture/L2-crate-topology §1.3 | Crate `posthaste-link-contract` with `DownFrame` (`lib.rs:130`), `RuntimeId` (`lib.rs:181`); crate doc overclaims "the contract both links speak"; crate `posthaste-authority-runtime` with `BackendApi`/`BackendLink`/`BackendNode`/`LocalBackend`; `RemoteBackend`/`RuntimeBackendOutbox` in `posthaste-runtime`; bins `posthaste_backend`, `posthaste_daemon`, `posthaste_runtime_daemon` | Crate `posthaste-authority-server-link` with `AuthorityServerFrame`, `AuthorityServerLinkId`, `AuthorityServerApi`, `AuthorityServerLink`; crate `posthaste-authority-server` with `AuthorityServerNode`, `LocalAuthorityServer`; `RemoteAuthorityServer`/`RuntimeAuthorityServerOutbox` in `posthaste-runtime`; bins `posthaste-authority-server`, `posthaste-authority-runtime-server` (bundled), `posthaste-runtime`, `posthaste-client` (D18); doc scoped to the authority-server-link surface | open |
| V2 | D5, D6, D8, D11, D12, D16 | architecture/L2-crate-topology §1.1; runtime/adapter; replication/* | No `posthaste-contract-core`. Wire types fused with `RuntimeCore` in `runtime-contract` (`lib.rs:68-1354`); stringly `MutationRequest{name,args}` (`lib.rs:730`); typed `MessageMutation` stranded in `link-contract/message_mutation.rs:27`; `WireMutationId` mirror alive (`link-contract/lib.rs:147`); dead settlement arms (`Conflict`/`Queued`/`LocalApplied`) | `posthaste-contract-core` crate: typed `MailOperation` (one vocabulary incl. the `revCursor` control arm, parsed once per wire at the §6.6 crossings), `MutationRequest`/`Receipt`, ids, view models, `RuntimeAdapterError`; `WireMutationId` deleted (`link_core::MutationId` is the one wire id); settlement vocabulary trimmed to live states; the 5 per-command RPCs collapsed into one `apply_mail_operation(MailOperation) -> CommandAck` (D21 — one typed entry per semantics) | landing — M2 landed (`rzponpsl`, 2026-07-02): contract-core exists, wire types moved, wasm-clean; runtime-contract is a slim trait+shim awaiting M3; remaining (MailOperation typing, WireMutationId deletion, settlement trim, D21 collapse) all land at M5 |
| V3 | D7 | runtime/internals L1–L3; architecture/L2-crate-topology §1.3 | `RuntimeCore` god-trait, ~60 methods (`runtime-contract/lib.rs:1011-1354`), implemented by `posthaste-runtime`; near node already moved to `crates/posthaste-runtime` while old specs said `authority-runtime` | Split: `posthaste-runtime-api` (typed wire-free domain RPC) + `posthaste-client-link` (link ops: mutation forward, frame stream, session/view ops) | open |
| V4 | D10, D13 | backend/L2; architecture/L2-crate-topology §1.1–1.2 | `posthaste-domain` god-crate: 13 modules, model+ports+MailService+provider+imap+cache+search, `mail-parser`+`tokio` deps; `ConfigError` in `config.rs`, `ValidationError` in `validation.rs`; every wire consumer drags all of it | `posthaste-domain-model` (pure types incl. relocated `ConfigError`/`ValidationError`, per RFC §6.1 file assignment as amended by D30) + `posthaste-domain-service` (hexagonal core); no glob re-export shim in end-state | landing — M1 landed (`zuyxovxr`, 2026-07-02): split done, gates green, model wasm-clean; remaining: the temporary glob shim (sunset M8) |
| V5 | D14 | architecture/L2-crate-topology §4 `no-parallel-namespaces` | `posthaste-store` re-exports ~80 domain symbols wholesale | `store` exports only what it owns (`DatabaseStore`, `RepairReport`, …); consumers import domain types from the domain crates | open |
| V6 | D15 | architecture/L2-crate-topology §3 | M0 landed (`pkrvlwqm`, 2026-07-02): `wasm-frontier` CI job checks `link-core` + `link-replica`. Remaining: frontier still breached via `link-wasm → runtime-contract → domain → {mail-parser, tokio}`; `domain-model`/`contract-core` don't exist yet | `link-core`, `link-replica`, `domain-model`, `contract-core` build for `wasm32-unknown-unknown` in CI | landing |
| V8 | D19 | architecture/L2-crate-topology §4 `no-parallel-namespaces` | `posthaste-authority-runtime/lib.rs:39-43` re-exports 10 near-node symbols (4 with zero shim consumers; `RuntimeHandle`/`BackendTransportConfig` under two namespaces; `SystemSecretStore` under three); `posthaste-server/lib.rs:11-16,23-29` glob-facades the whole api surface + dead `pub mod oauth` + tests-only `pub mod supervisor` | No cross-crate facade re-exports anywhere; consumers declare the owning crate (server/testkit/bench add a `posthaste-runtime` dep; server tests import deps directly) | open |
| V9 | D20 | authority-server/L3 intro; runtime/internals/L3 intro | `MigrationRuntime`, 2 dead `from_api_bridge_*` constructors, zero-consumer provider traits (`account_reads.rs`/`live_accounts.rs`), `AuthorityRuntimeApiMigrationBridge`, `server/src/migration.rs` (57 LOC) live in prod surfaces; code `@spec` comments point at the retired wrapper-migration plan | Dead symbols deleted; test bridge in testkit/`cfg(test)` only; no dangling `@spec` pointers | open |
| V10 | D24 | architecture/L2-crate-topology §1.3–1.4; replication/authority-server-link L2 | `link_router`/`start_backend` live in `posthaste-server/link.rs` and borrow `ApiError` + 3 auth helpers from `posthaste-api` (`link.rs:40-41`); the standalone far-node binary drags the /v1 client platform | The far-node link wire lives in `posthaste-authority-server` with its own minimal error/auth vocabulary; `posthaste-server` only mounts it | open |
| V11 | D25, D26, D27 | authority-server/L2 §1.1; api/L2 §1 | `DaemonSettings` resolution in `posthaste-api/src/config.rs:115-207` field-reads the voldemort `AppToml` from `read_app_toml()`; `DaemonRuntimeTuning` + 5 sub-structs exported with zero consumers; node assembly + fail-closed `LinkAuth` triplicated (`startup.rs` ≡ `startup_backend.rs` ≡ `runtimed/main.rs`) | `posthaste-config` owns settings resolution; no voldemort types cross the boundary; dead tuning surface deleted; one shared assembly helper + one `LinkAuth::from_daemon_settings` | landing — D26 landed (`ospkxvwk`) + D25 landed (`mtstxsrn`, config owns daemon-settings resolution, `read_app_toml` sealed) — both hygiene ws, 2026-07-02; D27 (assembly dedup) remains, sequenced after M3b clears `link.rs`/startups |
| V12 | D28 | state/mail/L2 (query evaluator) | Two tokenizers for one grammar: `authority-server mail_queries/rules/tokenize.rs` (190 LOC) pre-splits `in:` then re-feeds `domain search::parse_query` (`rules.rs:36`) | One tokenizer in domain-service with an `in:`-resolver hook | open |
| V7 | D9 | replication/backend-link L2; replication/client-link L2 | `Replica`/`Link`/`Outbox` layering ~80% realized in structure (`MessageReplica`, `RuntimeBackendOutbox`, `EntityStore::settle`) but not legible in trait seams; race-free retire invariant lives only in `EntityStore::settle`, not the engine (stale issue L2-engine-absorption-footguns) | Explicit `Replica` (and if warranted `Link`/`Outbox`) trait views over the single owner; the second convergence consumer (runtime near node) uses the same seam | open |

## Conventions

- One row per **divergence**, not per file: a single crate split that touches
  40 call sites is one row.
- `Current state` must name the crates/types as they exist today (old names),
  `End state` uses the spec's names — the row is the old↔new bridge while both
  exist.
- A row may not close while any `[::state partial …]` marker it owns remains
  in a durable spec.
