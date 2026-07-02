---
scope: L2
summary: "Crate topology — the one place the workspace's crate set, ownership, dependency hierarchy, role binaries, and wasm-pure frontier are named. End-state per RFC-L2-architecture-cleanup; sections whose code lags carry [::state] markers."
modified: 2026-07-02
reviewed: 2026-07-02
state: planned
depends:
  - path: docs/replication/L1
  - path: docs/runtime/internals/L1
  - path: docs/authority-server/L2
dependents:
  - path: docs/authority-server/L2
  - path: docs/replication/authority-server-link/L2
  - path: docs/replication/client-link/L2
---

# Crate topology

This spec names the workspace boundary once (XV): which crates exist, what each
owns, the dependency direction, the role binaries, and the wasm-pure frontier.
`authority-server/L2 §1.1` defers to this table for the workspace-wide picture
and keeps only the authority-server-tier detail.

Naming rules (XXII): a crate is named by **what it owns**; where a construct
has an emitter/tier, by its emitter (`AuthorityServerFrame`, `RuntimeFrame`).
The far-node component has exactly one canonical name — **authority server** —
replacing both "backend" and "authority runtime" (one component, one name;
"backend" stays available for generic uses like backing stores). A name that
overclaims ("the contract both links speak") is treated as a bug. A suffix
carries exactly one meaning everywhere it appears (RFC D32): **`*Api`** = typed
wire-free RPC surface (`RuntimeApi` + its four subtraits); **`*Link`** =
replication-link contract (a coherent-link seam: mutation forward + frame
subscribe + read-through); **`*Handle`** = owning wrapper (`RuntimeHandle`
pattern); **`*Adapter`** = protocol translation (`HttpApiAdapter`).

## 1. The crate set (end-state)

[::state partial plan=eph/RFC-L2-architecture-cleanup]
*The splits and renames below are drained intent; code still has `posthaste-domain`
(unsplit), `posthaste-runtime-contract` (fused), `posthaste-link-contract` and
`posthaste-authority-runtime` (old names, `Backend*` types). See
DEVIATION-L2-architecture-cleanup rows V1–V6.*

### 1.1 Shared vocabulary tier (wasm-pure)

| Crate | Owns | May depend on |
|---|---|---|
| `posthaste-link-core` | The effect-fold leaf: `MessageFoldState` predictor, convergence engine (`Replica`, `MessageReplica`, `MutationId`, settlement fold). *Below* domain; domain-free by construction. | — |
| `posthaste-link-replica` | The keyed reactive store over link-core: `EntityStore`, view rows/predicates, retirement draining. | link-core |
| `posthaste-domain-model` | The pure domain types: ids, messages, records, commands, outbox/sync/rev-log types, smart mailboxes, account settings/overview, appearance, automation, notifications, vocab (`MailboxRole`, `SystemKeyword`), errors (`GatewayError`, `StoreError`, `ServiceError(Kind)`, `SecretStoreError`, `ConfigError`, `ValidationError`), plus the pure cache/imap/provider slices the model types' inherent impls close over: cache primitives/entities/budget, imap types/sync-state/capabilities/mailbox-roles, and the whole provider profile+policy set (RFC D30). | — |
| `posthaste-contract-core` | The shared wire vocabulary *above* domain-model: the typed `MailOperation` enum (the one operation vocabulary, parsed once per wire), `MutationRequest`/`MutationReceipt`, `MutationSettlementState`, opaque ids (`RuntimeSessionId`, `ViewId`, `ClientMutationId`, `RuntimeMutationId`, `ViewRevision`), view models (`RuntimeFrame`, `ViewFrame`, `ViewSnapshot`, `MailListViewState`, `CoverageRange`), `RuntimeAdapterError` (+ `From<ServiceErrorKind>`), `mutation_args`, `mail_query`. | domain-model, link-core |

### 1.2 Domain service tier

| Crate | Owns | May depend on |
|---|---|---|
| `posthaste-domain-service` | The hexagonal core: `MailService`, all port traits (`MailGateway`, `MailStore` composite, secret/config/push ports), imap planning + identities logic, cache scoring/governor, search parsing, `validate_*` functions. (Provider *policies* are model-resident data per RFC D30; the service owns the behavior consuming them.) Forwards the `openapi` feature to domain-model. | domain-model, link-core, observability |

### 1.3 Link surface tier

| Crate | Owns | May depend on |
|---|---|---|
| `posthaste-authority-server-link` | The runtime↔authority-server seam, mirroring the client↔runtime seam's shape (RFC D33): **`AuthorityServerApi`** (the typed request surface — reads, account/settings ops, `apply(op: MailOperation)`) + **`AuthorityServerLink`** (the coherent-link mechanics — `forward_mutation`, `subscribe(coverage)` → frames, settlement/watermark, outbox op-lifecycle) + `AuthorityServerLinkHandle` (wrapper), `AuthorityServerFrame` (base assertions + settlement), `AuthorityServerLinkId`, `LinkCoverage`, `LINK_*_PATH`, generated request structs. No shared vocabulary lives here (that is contract-core). *(Split pending: one fused trait until M5b — [::state] per §1 marker.)* | contract-core, domain-model, link-core |
| `posthaste-runtime-api` | The typed, wire-free client-facing domain RPC extracted from `RuntimeCore` (41 of its 52 methods): returns serde domain types, no frames. **Four traits** — `RuntimeAccountApi`, `RuntimeSettingsApi`, `RuntimeMailReadApi`, `RuntimeMailWriteApi` (whose message commands are one typed `apply(op: MailOperation) -> CommandAck` entry (D34), not per-command RPCs) — plus an umbrella supertrait; narrow consumers take one trait (`&dyn RuntimeAccountApi`). | contract-core, domain-model |
| `posthaste-client-link` | The client↔runtime link ops extracted from `RuntimeCore` (**one trait, `RuntimeLink`**, 10 methods): `forward_mutation` (the up-channel flush, one verb across both seams per D35), the three stream families (`subscribe_runtime_frames`, `subscribe_events`, session-view snapshots), session open/close, view open/extend/close. | contract-core, domain-model, link-core |

### 1.4 Node/adapter tier (native)

| Crate | Owns | May depend on |
|---|---|---|
| `posthaste-store` | SQLite adapter (`DatabaseStore`, `RepairReport`). Exports only what it owns — no re-exports of domain symbols. | domain-service (+model) |
| `posthaste-engine` | JMAP gateway/push adapters. | domain-service |
| `posthaste-imap` | IMAP gateway adapter. | domain-service |
| `posthaste-config` | TOML config persistence (`TomlConfigRepository`, tuning schemas). | domain-service (types via domain-model) |
| `posthaste-runtime` | The near node: runtime assembly, outbox (`RuntimeAuthorityServerOutbox` over `MessageReplica`), `ReadCache`, the remote authority-server transport (`RemoteAuthorityServer`), implements runtime-api + client-link. | runtime-api, client-link, authority-server-link, link-replica, link-core, domain-service |
| `posthaste-authority-server` | The far node: `AuthorityServerNode`, account supervision, sync, push, oauth, `AuthorityServerLink` impls (`LocalAuthorityServer`), registry, **and its own link wire** (`link_router` + link auth — the far node owns the surface it serves; it does not borrow the /v1 platform's error/auth vocabulary). No re-exports of near-node symbols. | authority-server-link, runtime, domain-service, engine, imap, store, config |
| `posthaste-link-wasm` | The wasm client replica binding (JSON boundary). | link-core, link-replica, domain-model, contract-core |
| `posthaste-http-api-adapter` | The HTTP API adapter: serves the /v1 contract over the runtime's typed Api surfaces. | runtime-api, client-link, domain-service, config |
| `posthaste-server` | Composition root: assembles nodes and mounts routers (http-api-adapter's `/v1`, authority-server's link wire), HTTP serving. No facade re-exports; no logic of its own beyond assembly. | http-api-adapter, authority-server, … |
| `posthaste-runtimed` | Runtime daemon crate. | http-api-adapter, runtime |
| `posthaste-observability`, `posthaste-testkit`, `posthaste-bench`, `posthaste-lab`, `posthaste-wizard` | Telemetry, test harness, benches, tooling. | (tier-appropriate) |

### 1.5 Role binaries

[::state partial plan=eph/RFC-L2-architecture-cleanup]
*Current bins: `posthaste_backend`, `posthaste_daemon` (posthaste-server),
`posthaste_runtime_daemon` (posthaste-runtimed); desktop app unnamed as a role
bin (RFC D18).*

A binary is named after the component it runs — no more, no less:

| Binary | Runs | Ships from |
|---|---|---|
| `posthaste-authority-server` | The far node, standalone. | posthaste-server crate |
| `posthaste-authority-runtime-server` | The bundled all-in-one: authority server + near-node runtime + API, colocated behind one HTTP server. The name enumerates the bundled components; it does not revive "authority runtime" as a component name. | posthaste-server crate |
| `posthaste-runtime` | The near-node runtime daemon. | posthaste-runtimed |
| `posthaste-client` | The desktop client app. | apps/desktop |

Tool bins (`posthaste-wizard`, `posthaste-lab`, `posthaste-profile`) keep their
names. Bin names are hyphenated, never underscored.

## 2. Dependency direction

```
link-core ──────────┐
link-replica ───────┤            (wasm-pure tier)
domain-model ───────┼─► contract-core
                    │
domain-service ─────┘            (service tier)
        │
        ├─► store / engine / imap / config          (adapters)
        │
contract-core ─► authority-server-link              (link surfaces)
contract-core ─► runtime-api + client-link
        │
        └─► runtime ─► authority-server ─► server   (nodes, roots)
                 └────► link-wasm / http-api-adapter / runtimed
```

Rules:

- **No upward edges.** A wire/link crate never depends on a node crate; a
  vocabulary crate never depends on a wire crate; nothing depends on a
  composition root.
- **The up-vocabulary lives above domain; the effect fold below it.**
  `link-core` stays the narrow domain-free leaf (the `domain → link-core →
  domain` cycle is avoided by the minimal `MessageFoldState` predictor);
  `contract-core` carries the wire vocabulary and may reference domain-model.
- **One operation vocabulary.** `MailOperation` is defined once in
  contract-core, parsed once per wire crossing, carried typed inward; dispatch
  is an exhaustive match. No crate re-mirrors ids (`WireMutationId` is deleted;
  `link_core::MutationId` is the one wire id).

### 2.1b Node anatomy (D36–D39)

Every node is a composition of these parts — shared parts have exactly one
implementation; bracketed parts are mounts, not forks:

```
node = OptimisticReplica (kernel, link-core)              ← shared
     + Projector (windowed views, link-replica)           ← shared (D38)
     + link near-end (Link trait + transport)             ← every near node
     [+ link far-end (sessions/frames/registry/wire)]     ← only fan-in nodes (D37, D39)
     [+ UI composition (reactivity, persistence)]         ← client only (D36)
     [+ Evaluator + providers (MailService)]              ← authority server only (D38)
```

client = kernel + projector + near-end(`RuntimeLink`) + UI.
runtime = kernel + projector + near-end(`AuthorityServerLink`) + far-end(serves clients).
authority server = evaluator + providers + far-end(serves runtimes).

The *evaluator* (query → membership over unbounded mail) is authority-only by
the windowed-view-replica product decision; the D15 frontier CI proves the
client closure cannot contain it. The *projector* (rows + coverage + pending →
windowed views) is one component both near nodes mount. A headless client is
kernel + projector + near-end — no UI mount required.

### 2.1 Replica seams

[::state partial plan=eph/RFC-L2-architecture-cleanup]
*Trait seams not yet extracted (RFC D9); the retire invariant still lives only
in `EntityStore::settle`.*

The Link/Replica/Outbox layering is legible **in types**, not only in
structure: `link-core` exposes an explicit `Replica` trait (and `Link`/`Outbox`
views only if a caller benefits) over the single-owner `MessageReplica`. There
is exactly one store — base + pending, optimism folded on read; the seams are
*views* over that owner, never a second copy (a split store was considered and
rejected). The version-gated race-free retire invariant lives in the engine
seam itself, so both convergence consumers — the client `EntityStore` and the
runtime near node's `RuntimeAuthorityServerOutbox` — inherit it rather than
re-implementing it.

## 3. The wasm-pure frontier

[::state partial plan=eph/RFC-L2-architecture-cleanup]
*CI enforcement does not exist yet (RFC D15).*

`link-core`, `link-replica`, `domain-model`, `contract-core` are serde-only: no
`tokio`, `reqwest`, `rusqlite`, `mail-parser`, `axum`, `uuid`-free except
domain-model's `generated_id`. The frontier is CI-enforced (`cargo check
--target wasm32-unknown-unknown` for the four crates); a frontier nobody checks
is a hope, not a boundary (XXIV). The wasm client's full dependency closure is
exactly these four crates plus `link-wasm` itself.

## 4. Assertions

| id | assertion |
|---|---|
| `crate-named-by-ownership` | Every crate name states what the crate owns; link crates are named by the seam they carry (`authority-server-link`), frame types by their emitter (`AuthorityServerFrame`, `RuntimeFrame`). |
| `one-component-one-name` | The far node is the **authority server** everywhere — specs, types, crates, binaries; "backend" and "authority runtime" are not synonyms for it. |
| `binary-named-by-component` | Every role binary is named exactly after the component it runs (§1.5); hyphenated. |
| `no-upward-deps` | The dependency graph respects §2; adding an upward edge is a spec violation, not a Cargo.toml detail. |
| `one-operation-vocabulary` | The typed `MailOperation` in contract-core is the only operation vocabulary; no stringly `name`/`args` crosses a crate boundary. |
| `wasm-frontier-enforced` | The four wasm-pure crates build for `wasm32-unknown-unknown` in CI. |
| `no-parallel-namespaces` | No crate re-exports another crate's public surface (store's historical ~80-symbol domain re-export is the counterexample). Temporary migration shims carry an owner and a sunset. |
