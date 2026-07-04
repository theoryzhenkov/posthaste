---
scope: L2
summary: "Crate topology — the one place the workspace's crate set, ownership, dependency hierarchy, role binaries, and wasm-pure frontier are named. Realized per RFC-L2-architecture-cleanup M0–M9c plus RFC-L2-provider-reliability M30/M31 (the call-policy/provider-call split); no [::state] markers remain (§2.1b records the D38 projector-merge verdict: not a fit, not forced)."
modified: 2026-07-04
reviewed: 2026-07-04
state: active
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

*The crate set below is REAL as of 2026-07-04 (architecture-cleanup M0–M9c: the
domain/contract-core splits, the `RuntimeCore` and far-node trait splits, the
authority-server renames, the typed `MailOperation`, `OptimisticReplica`, the
frontier CI, the `LinkFarEnd`/`LinkNearEnd` engines, the opaque-id rename to
`RuntimeLinkId` + session→link-connection vocabulary (D42), and the `replica-core`/
`replica-projector` crate renames (D43); plus provider-reliability M30/M31: the
wasm-pure `posthaste-call-policy` and native `posthaste-provider-call` split).
Remaining lag (M9+): the D38 projector merge did not land — see §2.1b for the
verdict.*

### 1.1 Shared vocabulary tier (wasm-pure)

| Crate | Owns | May depend on |
|---|---|---|
| `posthaste-replica-core` | The effect-fold leaf: `MessageFoldState` predictor, convergence engine (`Replica`, `MessageReplica`, `MutationId`, settlement fold). *Below* domain; domain-free by construction. | — |
| `posthaste-replica-projector` | The keyed reactive store over replica-core: `EntityStore`, view rows/predicates, retirement draining. | replica-core |
| `posthaste-domain-model` | The pure domain types: ids, messages, records, commands, outbox/sync/rev-log types, smart mailboxes, account settings/overview, appearance, automation, notifications, vocab (`MailboxRole`, `SystemKeyword`), errors (`GatewayError`, `StoreError`, `ServiceError(Kind)`, `SecretStoreError`, `ConfigError`, `ValidationError`), plus the pure cache/imap/provider slices the model types' inherent impls close over: cache primitives/entities/budget, imap types/sync-state/capabilities/mailbox-roles, and the whole provider profile+policy set (RFC D30). | — |
| `posthaste-contract-core` | The shared wire vocabulary *above* domain-model: the typed `MailOperation` enum (the one operation vocabulary, parsed once per wire), `MutationRequest`/`MutationReceipt`, `MutationSettlementState`, opaque ids (`RuntimeLinkId`, `ViewId`, `ClientMutationId`, `RuntimeMutationId`, `ViewRevision`), view models (`RuntimeFrame`, `ViewFrame`, `ViewSnapshot`, `MailListViewState`, `CoverageRange`), `RuntimeAdapterError` (+ `From<ServiceErrorKind>`), `mutation_args`, `mail_query`. | domain-model, replica-core |
| `posthaste-query-grammar` | The one query grammar: the tokenizer + `parse_query`/`parse_query_with_scopes`/`ScopeToken` that compile human-readable search strings into `SmartMailboxRule` trees. Extracted out of domain-service (RFC-L2-scripting §7 ruling 4, D28) so both smart mailboxes (domain-service) and the rules engine's WHEN-clause grammar consume the same parser without the rules engine dragging in domain-service. Wasm-pure (frontier-capable; not frontier-listed — nothing mounts it at the wasm boundary yet). | domain-model |
| `posthaste-call-policy` | The shared outbound-call **policy core** (RFC-L2-provider-reliability D80–D82, M30): the wasm-pure arithmetic that decides *how* an outbound provider or link call retries, backs off, deadlines, and classifies — `BackoffSchedule` (full-jitter capped exponential + `Retry-After`/429 math + `max_attempts`), the per-`CallClass` `DeadlinePolicy` table, and `classify_status`/`resolve_terminality` over the shared `Terminality` taxonomy (owned by domain-model, consumed here, never re-minted). Dependency-thin and clock/RNG-free — every entry point takes explicit `now`/`attempt`/`rand_unit` inputs — so the link engine (`link-near-end`) and the native provider executor (`provider-call`) consume one shared fact rather than forking it (tenet XIV). Frontier-listed: it rides into the browser via `link-near-end` → `client-node-wasm`. | domain-model |

### 1.2 Domain service tier

| Crate | Owns | May depend on |
|---|---|---|
| `posthaste-domain-service` | The hexagonal core: `MailService`, all port traits (`MailGateway`, `MailStore` composite, secret/config/push ports), imap planning + identities logic, cache scoring/governor, `validate_*` functions. (Provider *policies* are model-resident data per RFC D30; the service owns the behavior consuming them.) Query parsing moved out to `posthaste-query-grammar`; consumers import it directly (no re-export here). Forwards the `openapi` feature to domain-model. | domain-model, replica-core, observability |

### 1.3 Link surface tier

| Crate | Owns | May depend on |
|---|---|---|
| `posthaste-authority-server-link` | The runtime↔authority-server seam, mirroring the client↔runtime seam's shape (RFC D33): **`AuthorityServerApi`** (the typed request surface — reads, account/settings ops, `apply(op: MailOperation)`) + **`AuthorityServerLink`** (the coherent-link mechanics — `forward_mutation`, `subscribe(coverage)` → frames, settlement/watermark, pending-set op-lifecycle) + `AuthorityServerLinkHandle` (wrapper), `AuthorityServerFrame` (base assertions + settlement), `AuthorityServerLinkId`, `LinkCoverage`, `LINK_*_PATH`, generated request structs. No shared vocabulary lives here (that is contract-core). | contract-core, domain-model, replica-core |
| `posthaste-runtime-api` | The typed, wire-free client-facing domain RPC extracted from `RuntimeCore` (41 of its 52 methods): returns serde domain types, no frames. **Four traits** — `RuntimeAccountApi`, `RuntimeSettingsApi`, `RuntimeMailReadApi`, `RuntimeMailWriteApi` (whose message commands are one typed `apply(op: MailOperation) -> CommandAck` entry (D34), not per-command RPCs) — plus an umbrella supertrait; narrow consumers take one trait (`&dyn RuntimeAccountApi`). | contract-core, domain-model |
| `posthaste-client-link` | The client↔runtime link ops extracted from `RuntimeCore` (**one trait, `RuntimeLink`**, 9 methods): `forward_mutation` (the up-channel flush, one verb across both seams per D35), the three stream families (`subscribe_runtime_frames`, `subscribe_events`, link-view snapshots), link open/close, link-view open/extend/close, `mutation_settlement` (the reconciler's cross-link settlement lookup, M9b2). The sessionless `open_view`/`subscribe_view` pair was deleted at M10 (D51: zero call sites). | contract-core, domain-model, replica-core |
| `posthaste-link-near-end` | The shared near-end engine (D40/D41): `Wire`-generic transport+resilience (deadlines, jittered reconnect, seq cursor, the level-triggered reconciler); the client↔runtime profile (`RuntimeLinkWire`) lives here, wasm-pure. | contract-core, domain-model |
| `posthaste-link-far-end` | The shared far-end engine (D40): composable sub-stores (dedup, settlement sinks, seq-backlog replay with collapse fallback + expiry) both far-ends (runtime, authority server) assemble. | contract-core, domain-model |

### 1.4 Node/adapter tier (native)

| Crate | Owns | May depend on |
|---|---|---|
| `posthaste-store` | SQLite adapter (`DatabaseStore`, `RepairReport`). Exports only what it owns — no re-exports of domain symbols. | domain-service (+model) |
| `posthaste-engine` | JMAP gateway/push adapters. Routes its outbound JMAP calls through `provider-call`. | domain-service, provider-call |
| `posthaste-provider-call` | The **native** outbound-call envelope (RFC-L2-provider-reliability D83, M31): the tokio/reqwest *executor* half over the wasm-pure `call-policy` core. Owns the shared `reqwest::Client` connection pool, applies the per-class deadline (`tokio::time::timeout` total, or a between-chunks *stall* read-deadline for blobs via `stall_guard`), runs the `Retry-After`-aware jittered retry loop, and holds the per-account circuit breaker (`ProviderCallExecutor`, `CallErrorReason::CircuitOpen`). Native-only by construction — it must never enter the wasm frontier; only `engine` depends on it. | call-policy, domain-model |
| `posthaste-imap` | IMAP gateway adapter. | domain-service |
| `posthaste-config` | TOML config persistence (`TomlConfigRepository`, tuning schemas). | domain-service (types via domain-model) |
| `posthaste-runtime` | The near node: runtime assembly, the far-end link registry (`LinkRegistry`, RFC D42), pending set (`AuthorityServerPendingSet` over `MessageReplica`), `ReadCache`, the remote authority-server transport (`RemoteAuthorityServer`), implements runtime-api + client-link. | runtime-api, client-link, authority-server-link, link-far-end, replica-projector, replica-core, domain-service |
| `posthaste-authority-server` | The far node: `AuthorityServerNode`, account supervision, sync, push, oauth, `AuthorityServerLink` impls (`LocalAuthorityServer`), registry, **and its own link wire** (`link_router` + link auth — the far node owns the surface it serves; it does not borrow the /v1 platform's error/auth vocabulary). No re-exports of near-node symbols. | authority-server-link, runtime, domain-service, query-grammar, engine, imap, store, config |
| `posthaste-client-node-wasm` | The wasm client node assembly (D41/D43): kernel (replica-core) + projector (replica-projector) + near-end (`posthaste-link-near-end`, wasm32-only cfg), exposed as a wasm-bindgen JSON boundary. | replica-core, replica-projector, link-near-end, domain-model, contract-core |
| `posthaste-http-api-adapter` | The HTTP API adapter: serves the /v1 contract over the runtime's typed Api surfaces. | runtime-api, client-link, domain-service, config |
| `posthaste-server` | Composition root: assembles nodes and mounts routers (http-api-adapter's `/v1`, authority-server's link wire), HTTP serving. No facade re-exports; no logic of its own beyond assembly. | http-api-adapter, authority-server, … |
| `posthaste-runtimed` | Runtime daemon crate. | http-api-adapter, runtime |
| `posthaste-observability`, `posthaste-testkit`, `posthaste-bench`, `posthaste-lab`, `posthaste-wizard` | Telemetry, test harness, benches, tooling. | (tier-appropriate) |

### 1.5 Role binaries

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
replica-core ────────┐
replica-projector ───┤            (wasm-pure tier)
domain-model ───────┼─► contract-core
        │           │
        └─► call-policy                  (wasm-pure policy core)
                    │
domain-service ─────┘            (service tier)
        │
        ├─► store / engine / imap / config          (adapters; engine ─► provider-call)
        │
contract-core ─► authority-server-link              (link surfaces)
contract-core ─► runtime-api + client-link
contract-core ─► link-near-end / link-far-end        (shared link-end engines)
        │
        └─► runtime ─► authority-server ─► server   (nodes, roots)
                 └────► client-node-wasm / http-api-adapter / runtimed
```

Rules:

- **No upward edges.** A wire/link crate never depends on a node crate; a
  vocabulary crate never depends on a wire crate; nothing depends on a
  composition root.
- **The up-vocabulary lives above domain; the effect fold below it.**
  `replica-core` stays the narrow domain-free leaf (the `domain → replica-core →
  domain` cycle is avoided by the minimal `MessageFoldState` predictor);
  `contract-core` carries the wire vocabulary and may reference domain-model.
- **One operation vocabulary.** `MailOperation` is defined once in
  contract-core, parsed once per wire crossing, carried typed inward; dispatch
  is an exhaustive match. No crate re-mirrors ids (`WireMutationId` is deleted;
  `replica_core::MutationId` is the one wire id).

### 2.1b Node anatomy (D36–D39)

Every node is a composition of these parts — shared parts have exactly one
implementation; bracketed parts are mounts, not forks:

```
node = OptimisticReplica (kernel, replica-core)            ← shared
     + Projector (windowed views, replica-projector)       ← shared (mechanism/projection layers)
     + link near-end (Link trait + transport)               ← every near node (posthaste-link-near-end)
     [+ link far-end (links/frames/registry/wire)]          ← only fan-in nodes (posthaste-link-far-end, D37, D39)
     [+ UI composition (reactivity, persistence)]           ← client only (D36)
     [+ Evaluator + providers (MailService)]                ← authority server only (D38)
```

client = kernel + projector + near-end(`RuntimeLink`) + UI.
runtime = kernel + projector + near-end(`AuthorityServerLink`) + far-end(serves clients).
authority server = evaluator + providers + far-end(serves runtimes).

The *evaluator* (query → membership over unbounded mail) is authority-only by
the windowed-view-replica product decision; the D15 frontier CI proves the
client closure cannot contain it. The *projector* (rows + coverage + pending →
windowed views) is one component both near nodes mount **for the fold-pending-
over-served-rows half** (`replica-projector`'s `mechanism`/`projection`
modules, wasm-pure, JSON-native). D38's verify-and-lift landed the shared
`LinkFarEnd`/`LinkNearEnd` engines and the D42 rename, but found the runtime's
`views.rs` (base-row read + family dispatch: `ViewKind`, `build_snapshot`,
`mail_list_state`) is **not** a second mount of the same component — it is the
authoritative *read* step, which has no client-side analog (the client only
ever reconciles already-served rows) and would need `replica-projector` to grow
new dependencies (contract-core, domain-model, an async read trait mirroring
most of `AuthorityServerApi`) it does not otherwise carry. `views.rs` stays
runtime-native; see its file header for the recorded verdict. A headless client
is kernel + projector + near-end — no UI mount required.

### 2.1 Replica seams

The Link/Replica/PendingSet layering is legible **in types**, not only in
structure: `replica-core` exposes the explicit `OptimisticReplica` trait (D35a;
`Link`/`PendingSet` views only if a caller benefits) over the single-owner
`MessageReplica`. There
is exactly one store — base + pending, optimism folded on read; the seams are
*views* over that owner, never a second copy (a split store was considered and
rejected). The version-gated race-free retire invariant lives in the engine
seam itself, so both convergence consumers — the client `EntityStore` and the
runtime near node's `AuthorityServerPendingSet` — inherit it rather than
re-implementing it.

## 3. The wasm-pure frontier

`replica-core`, `replica-projector`, `domain-model`, `contract-core`,
`posthaste-call-policy`, `posthaste-link-near-end` are serde-only: no `tokio`,
`reqwest`, `rusqlite`, `mail-parser`, `axum`, `uuid`-free except domain-model's
`generated_id`. The frontier is CI-enforced (`cargo check --target
wasm32-unknown-unknown` for the six crates plus `posthaste-client-node-wasm`
itself — the seven-crate frontier list; `call-policy` joined at
RFC-L2-provider-reliability M30); a frontier nobody checks is a hope, not a
boundary (XXIV). The wasm client's full dependency closure is exactly these six
crates plus `client-node-wasm` itself (D41: the near-end engine now compiles
into the wasm boundary, `cfg(target_arch = "wasm32")`-gated).

## 4. Assertions

| id | assertion |
|---|---|
| `crate-named-by-ownership` | Every crate name states what the crate owns; link crates are named by the seam they carry (`authority-server-link`), frame types by their emitter (`AuthorityServerFrame`, `RuntimeFrame`). |
| `one-component-one-name` | The far node is the **authority server** everywhere — specs, types, crates, binaries; "backend" and "authority runtime" are not synonyms for it. |
| `binary-named-by-component` | Every role binary is named exactly after the component it runs (§1.5); hyphenated. |
| `no-upward-deps` | The dependency graph respects §2; adding an upward edge is a spec violation, not a Cargo.toml detail. |
| `one-operation-vocabulary` | The typed `MailOperation` in contract-core is the only operation vocabulary; no stringly `name`/`args` crosses a crate boundary. |
| `wasm-frontier-enforced` | The seven wasm-pure-or-wasm-target frontier crates (six serde-only + `client-node-wasm`) build for `wasm32-unknown-unknown` in CI. |
| `no-parallel-namespaces` | No crate re-exports another crate's public surface (store's historical ~80-symbol domain re-export is the counterexample). Temporary migration shims carry an owner and a sunset. |
