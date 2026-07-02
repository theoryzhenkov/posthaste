---
scope: L2
summary: "RFC — architecture cleanup: the drain/outbox for a large navigability refactor of the replication/runtime crate topology. Holds the decision log (crate split, typed MailOperation vocabulary, frame/id renames, replica/link/outbox trait seams) and the name inventory. Decisions accumulate here as proposed → accepted → drained; nothing touches durable specs until drained, nothing touches code until its spec + DEVIATION row exist. Companion: DEVIATION-L2-architecture-cleanup (reality ledger)."
modified: 2026-07-02
reviewed: 2026-07-02
lifecycle: ephemeral
type: RFC
state: planned
depends: []
dependents: []
---

# RFC — Architecture Cleanup (crate topology, typed operations, link/replica seams)

Status: **drained — ready to execute** (`state: planned`; code untouched). This
is the **drain/outbox** for a large navigability refactor. It is the single
place refactor decisions accumulate; they drain into durable specs before any
code is touched.

Phase status (2026-07-02): **0 Scaffold ✓** · **1 Inventory ✓ — per-item
complete, all 10 ledger rows ☑** (§5, evidence §6.1–§6.8) · **2 Ratify ✓**
(Q1–Q9 resolved; no `proposed` rows; D19–D29 ratified from the per-item pass) ·
**3 Drain ✓** (revised specs at `docs/{architecture,replication,runtime,
authority-server,client,state,api,testing,ui}/`; DEVIATION rows V1–V12 open;
stale tree preserved at `docs/stale/`; naming waves D17/D18 swept) ·
**4 Sequence ✓** (§8, M0–M8 incl. M3b) · **5 Execute — pending**.

## 0. Purpose & the four-role discipline

The refactor is run over four artifacts with non-overlapping roles (per
[the process](#1-process)):

- **This RFC** — where *thinking* accumulates and drains. Decision log + name
  inventory below.
- **Durable specs** (`docs/replication/**`, `docs/runtime/**`, …) — carry
  *intent* (end-state). Edited only when a decision is `accepted`.
- **[`DEVIATION-L2-architecture-cleanup`](DEVIATION-L2-architecture-cleanup.md)**
  — carries *reality* (current-vs-end). One row per drained divergence.
- **`[::state …]` markers** — the *lag indicator* on any spec section whose code
  hasn't caught up.

Load-bearing rule: **specs carry intent, the DEVIATION register carries reality,
and reality is never edited back into a spec.**

## 1. Process

Phases (see the conversation of 2026-07-01 for the full rationale):

0. **Scaffold** — this doc + the DEVIATION shell. *(this commit)*
1. **Inventory & drain** — walk the crate graph **bottom-up**
   (`domain → link-core → contract-core → {backend-link, client-link, runtime-*}
   → server/runtime/wasm`), giving every public item a row here. Everything lands
   `proposed`.
2. **Ratify** — read the RFC whole for coherence (naming rule holds everywhere;
   no upward deps; no contradictions). Coherent rows → `accepted`.
3. **Drain into specs** — edit durable specs to end-state (bump `modified`, add
   `[::state partial plan=…]` where code lags), open the matching DEVIATION row,
   flip the RFC row → `drained`. Batch by domain to control staleness.
4. **Sequence the code** — derive the ordered migration plan (bottom-up), each
   step pointing at its spec section + DEVIATION row.
5. **Execute per unit** — DEVIATION row = current+end; land it; close the row;
   flip the `[::state]` marker; bump `reviewed`.

Invariants: nothing touches a durable spec until its decision is `accepted`;
nothing touches code until its row is `drained` **and** has a DEVIATION entry.
Rejected ideas stay as `rejected` rows **with rationale** — never deleted, so
they are not re-derived.

Status lifecycle: `proposed → accepted → drained` (or `rejected`).

## 2. Decision log

Seeded from the 2026-07-01 design conversation. Tenet refs point at
`~/.claude/agent/docs/engineering/L1-principles.md`.

| Ref | Decision | Rationale (tenet) | Status |
|-----|----------|-------------------|--------|
| D1  | `DownFrame → BackendFrame` — name the backend→runtime base frame by its emitter, consistent with `RuntimeFrame` (also emitter-named). Up-channel stays `MutationRequest`/`Receipt` (request/response, **not** a frame). | XXII (name reveals emitter/tier); XX (no speculative up-"frame") | drained |
| D2  | Do **not** introduce `ClientFrame` / `RuntimeBackendFrame`. The up-channel is one shared `MutationRequest` forwarded through both hops, not two per-emitter frames. | XXII (misleading name worse than none); XX (invents structure) | drained |
| D3  | `RuntimeId → BackendLinkId` — it is the per-connection backend-link identity, not "the runtime's own id." | XXII | drained |
| D4  | Rename crate `posthaste-link-contract → posthaste-authority-server-link`; fix its crate doc that overclaims "the contract both links speak" (it holds only the backend-link surface). | XXII; XV (name the boundary once) | drained |
| D5  | Extract `posthaste-contract-core` — the shared **wire** types (command set, view model, ids, `MutationRequest`/`Receipt`, errors) currently marooned in `runtime-contract`, so both link surfaces depend on it without dragging `RuntimeCore`. Keystone of the split. **Evidence (§6.2, 2026-07-01 traversal):** of all `runtime-contract` dependents, only `posthaste-api`, `posthaste-server` (oauth routes) and test/bench harnesses use the `RuntimeCore` trait; `link-wasm` (`mutation.rs:15` — one type), `link-contract` (`lib.rs:45` — types only), and most of `authority-runtime` import types only. The crate also violates its own "transport-neutral" doc by dragging `mail-parser`+`tokio` via the full `domain` dep. `contract-core` depends on `domain-model` (D10) + `link-core` (D12). | XIV (unify genuinely-shared facts); locality | drained |
| D6  | The shared up-vocab (`MutationRequest`/`Receipt` + ids + `MutationSettlementState` + `RuntimeAdapterError`) goes **into `contract-core` (above domain), not into `link-core`**. Q1 resolved: the up-vocab is wasm-pure (no blocker to link-core), **but** `link-core` is the effect-fold *below* domain, while the up-vocab is wire vocabulary *above* domain — merging would split the wire vocabulary across two layers (XIV) and strand the `From<ServiceErrorKind>` impl (which needs domain). `link-core` stays the narrow effect-fold leaf. Sealed by D8: once `MutationRequest` carries the typed `MailOperation`, up-vocab and command set become one type → one crate (`contract-core`). | XIV (wire vocab is one shared fact); layering; XXI | drained |
| D10 | Extract **`posthaste-domain-model`** — the pure shared types (messages, accounts, ids, errors, commands) — as a leaf, leaving the hexagonal core (`MailService` + ports + provider + imap + cache + search + …) as **`posthaste-domain-service`**. Today `domain` is a god-crate (13 modules, `mail-parser`/provider/imap) and every wire consumer (`runtime-contract`) drags all of it just to reach `AccountId`/`MessageSummary`. `contract-core` sits on the lean model, not the god-crate. **Not** "domain = data only" — the hexagonal port pattern is kept; only the pure types are lifted out. **Evidence (§6.1):** the internal seam already exists (`model/*`, `vocab`, `generated_id` vs `service/*`, `ports/*`, adapters); model-slice deps reduce to `serde`/`serde_json`/`thiserror`/`time`/`uuid`/`futures_util`/`utoipa(opt)`; exactly 2 of 12 dependents become model-only (`link-contract` cleanly; `runtime-contract` after D13). Three cross-module strands force pure-type files from `cache`/`imap`/`provider` into the model slice (§6.1c); the `openapi` feature moves to model and is forwarded by service. Naming: `-service` (not keeping bare `-domain`) — after the split a bare `posthaste-domain` no longer reveals which half it is (XXII). End-state: service does **not** glob-re-export model (a shim would hide the boundary the split exists to reveal); a temporary re-export with a sunset may stage the migration (XX). | XIV; XXI; locality; strengthens D5 | drained |
| D7  | Split `RuntimeCore`: typed domain RPC (wire-free, returns serde domain types) → `posthaste-runtime-api`; the link ops (`run_mutation` generic forward, `subscribe_runtime_frames` → `RuntimeFrame` stream, session/view-stream ops) → `posthaste-client-link`. This is what *earns* a wire-free type crate. | III (parse-once boundary); B3 (one trait, ~6 concerns) | drained |
| D8  | **Type the operation vocabulary** — replace stringly `MutationRequest { name: String, args: Value }` with a shared typed `MailOperation` enum, parsed once per wire, carried typed inward; dispatch becomes an exhaustive match, not a string lookup. The operation model is a **domain concern** every component (client/runtime/backend) imports. *Root fix of the "dirtiness."* *(Status cell was stale `proposed` until 2026-07-02 — contradicted Phase-2 ✓ and the drained §3 rows; corrected during the per-item pass.)* Wire-crossing map: §6.6. | III (parse, don't validate); XIV (shared fact) | drained |
| D9  | Extract explicit `Replica` (and possibly `Link` / `Outbox`) **trait seams** over the already-unified `MessageReplica`, so the Link/Replica/Outbox layering is legible in types (it is ~80% realized in structure today). *(Amended 2026-07-02 by D35: the trait is named `OptimisticReplica`.)* | XXII; XV | drained |
| D11 | **Q3 resolved — `MailOperation` supersedes `MessageMutation` and lives in `contract-core`.** `message_mutation::MessageMutation` (`link-contract/src/message_mutation.rs:27`) is already the pure typed dispatch table D8 wants; it is stranded in the backend-link crate, forcing `link-wasm` to depend on all of `link-contract` (trait + domain + runtime-contract) to reach it (§6.2). Move it to `contract-core`, absorb it into the typed `MailOperation` vocabulary (D8), and `link-wasm`'s `link-contract` dep likely evaporates (Q4). The `BackendApi` trait + backend wire frames stay in `authority-server-link` — the type/trait fusion there is cured by *removing the shared types*, not by another crate split (R3 still holds). | III; XIV; XXI | drained |
| D12 | **Delete `WireMutationId`** (`link-contract/src/lib.rs:147`) — a serde mirror of `link_core::MutationId` (`convergence.rs:13`) with `From` impls both ways, existing only because of crate-boundary placement. `MutationId` is already serde; `contract-core` depends on `link-core` (as `runtime-contract` does today) and carries `MutationId` directly. One fact, one type. | XIV; XXI | drained |
| D13 | **Relocate the `ConfigError` and `ValidationError` enums to `domain-model`** (from `config.rs` / `validation.rs`), keeping the `ConfigRepository` trait, `ConfigSnapshot`/`ConfigDiff`, and the `validate_*` functions in `domain-service`. Forced by the strand chain `ServiceError::Config(#[from] ConfigError)` (`model/errors.rs:64`) and `impl From<ValidationError> for ConfigError` (`config.rs:44,50`) — without this, `ServiceError` cannot sit in the model slice and neither `runtime-contract` nor `api`'s `From<ServiceErrorKind>` impl can go model-only (§6.1c). | III; XIV; unblocks D5/D10 | drained |
| D14 | **Remove `posthaste-store`'s ~80-symbol re-export of domain** (`DatabaseStore` + `RevLog*` + port traits re-exported wholesale). A parallel namespace for the same facts: consumers reading `posthaste_store::MailboxRecord` can't tell it is a domain type, and the shim drifts silently as domain grows. Consumers import from `domain-model`/`domain-service` directly; `store` exports only what it owns (`DatabaseStore`, `RepairReport`, …). | XIV (one namespace per fact); XXII | drained |
| D15 | **Declare the wasm-pure frontier as an invariant**: `link-core`, `link-replica`, `domain-model`, `contract-core` are serde-only (no `tokio`/`reqwest`/`rusqlite`/`mail-parser`/`axum`), CI-enforced (e.g. `cargo check --target wasm32-unknown-unknown` for the four crates, or a deny-list in a workspace lint/test). Today the frontier exists only by accident and was already breached once: `link-wasm → runtime-contract → domain → {mail-parser, tokio}` (§6.2d). An invariant nobody checks is a hope, not a boundary. | XXIV (encode the invariant as a test); XV | drained |
| D16 | **Trim the settlement vocabulary when typing the receipts (D8).** `MutationSettlementState`'s `Conflict`/`Queued`/`LocalApplied` arms are dead — only `Confirmed`/`Failed` ever settle (stale issue `L2-runtime-nearnode-remote-seam` item D). Likewise the operation set already shrank (`archive`/`trash`/`restoreToInbox` collapsed into `moveToRole`, issue `L2-sugar-role-mutations`, done) — `MailOperation` is defined from the *live* vocabulary, not the historical one. Verify the dead arms against code during Phase 4 before deleting. | XXI (remove the case before handling it); XX | drained |
| R1  | **Rejected** *(2026-07-01; superseded by D33 on 2026-07-02 — premises drifted)*: `posthaste-authority-server-api` (typed backend trait). `BackendApi` is all-link (no wire-free RPC behind it) and `subscribe → DownStream` returns a wire frame; the trait + wire go together. A typed backend would also force the runtime to decode+dispatch the mail vocabulary in a *second* place. *Why superseded: D8/D21 typed the vocabulary once-per-wire (forwarding a typed value is not a second dispatch; deserializing at the second network hop is boundary hygiene), and the traversal showed the trait's read half is a typed request surface in all but name (`ReadCache` mirrors it 1:1).* | XIV (would duplicate dispatch); XX; keeps the opaque relay | rejected |
| D17 | **Canonical component name: "authority server"** — replaces both "backend" (as the far-node component name) and "authority runtime" (2026-07-02, user-directed). One component, one name: "backend" was ambiguous (generic backends, backing stores) and "authority runtime" collided with the runtime tier. Supersedes the *names* chosen in D1/D3 (rationale unchanged — emitter/tier naming): `DownFrame → AuthorityServerFrame`, `RuntimeId → AuthorityServerLinkId`. Full map: crate `posthaste-authority-runtime → posthaste-authority-server`; `BackendApi → AuthorityServerApi`; `BackendLink → AuthorityServerLink`; `BackendNode → AuthorityServerNode`; `RemoteBackend/LocalBackend → RemoteAuthorityServer/LocalAuthorityServer`; `RuntimeBackendOutbox → RuntimeAuthorityServerOutbox` (name may be shortened at landing); spec dirs `docs/backend/ → docs/authority-server/`, `docs/replication/backend-link/ → docs/replication/authority-server-link/` (all `depends:`/`dependents:` paths follow); assertion ids containing `backend` follow the same rule. Generic uses of "backend" (e.g. "backing store", third-party backends) are NOT renamed — the rename targets the component. | XXII; XIV (one name per component) | drained |
| D18 | **Binaries are named after the component they run** (resolves Q5, 2026-07-02 user-directed): `posthaste_backend → posthaste-authority-server` (far node), `posthaste_runtime_daemon → posthaste-runtime` (near-node daemon; target-name collision with the library crate is acceptable — different artifact namespaces), desktop app binary → `posthaste-client`, `posthaste_daemon → posthaste-authority-runtime-server` (the bundled all-in-one, named by the composition it bundles: authority server + near-node runtime behind one HTTP server; amended from `posthaste-server` 2026-07-02, user-directed — note the name enumerates *components*, it does not reintroduce "authority runtime" as a component name, which D17 bans). Underscore bin names become hyphenated. Tool bins (`posthaste-wizard`, `posthaste-lab`, `posthaste-profile`) keep their names. | XXII | drained |
| D19 | **Kill every cross-crate facade re-export** — generalizes D14 into the invariant the topology spec already asserts (`no-parallel-namespaces`). (a) `posthaste-authority-runtime/lib.rs:39-43` re-exports 10 near-node symbols from `posthaste-runtime`; 4 have zero shim consumers; `RuntimeHandle`/`BackendTransportConfig` already live under two namespaces (api/runtimed import direct, server/testkit/bench via shim); `SystemSecretStore` is reachable by **three** paths. A far-node crate re-exporting near-node builders also violates `crate-named-by-ownership`. (b) `posthaste-server/lib.rs:11-16,23-29`: glob facade of the whole `posthaste-api` surface + `pub mod oauth` (zero consumers anywhere) + `pub mod supervisor` (consumed only by server's own test-support). Cure: consumers declare the owning crate (~8 use sites, 3 Cargo lines for (a); server tests import deps directly for (b)). | XIV; XXII | drained |
| D20 | **Delete the wrapper-migration residue** — the handler migration is 100% complete (zero `MailService`/`DatabaseStore` refs in `posthaste-api/src`; `posthaste-store` is dev-dep-only; all I/O is `state.runtime.*` — 63 call sites, §6.8). Dead now: `MigrationRuntime`, 2 of 4 `from_api_bridge_*` constructors, both zero-consumer provider traits (`account_reads.rs`/`live_accounts.rs`). Test-only: `AuthorityRuntimeApiMigrationBridge` + `server/src/migration.rs` (57 LOC) move to testkit/`cfg(test)`. The plan doc it pointed at no longer exists; durable-spec pointers scrubbed 2026-07-02, code `@spec` pointers scrubbed at landing. Delete pre-M4 so the rename doesn't churn corpses. | XXI; XX (expired scaffold) | drained |
| D21 | **One typed entry per command semantics — collapse the duplicated message-command wire path.** Today the same message commands cross the runtime↔authority-server link two ways with divergent semantics: 5 typed per-command RPCs (`set_message_keywords` et al. → `backend_link.set_keywords`, `build.rs:1008-1020`, consumer `api/message_commands.rs`) **bypass the outbox and `ClientMutationId` idempotency**, while `run_mutation` → typed dispatch (`backend.rs:934-1010`) gets optimism + dedup. Resolution: one vocabulary — the 5 per-command RPCs collapse into a single `apply_mail_operation(MailOperation) -> CommandAck` on `RuntimeMailWriteApi` (direct-apply semantics, *declared*: REST callers are not replicas and don't hold an outbox); the replica path keeps `run_mutation`. Both entries carry the same `MailOperation`; REST-retry idempotency is verified/added at M5 (X). D7 must not silently ratify the fork by placing one path per crate. | XIV; III; X | drained |
| D22 | **`MailOperation` covers control operations.** `revCursor` is live but sits *outside* `MessageMutation`, routed by string compare in two places (`posthaste-runtime/build.rs:561`, `backend.rs:934`) with per-site arg re-parsing. It becomes a typed variant of the M5 vocabulary; after M5 no stringly operation name survives anywhere (§6.6 crossing map). | III; XIV | drained |
| D23 | **D7 executed at method granularity (§6.4).** The 52 `RuntimeCore` methods split 41 → `runtime-api`, 9 → `client-link`, 2 dispositions: `subscribe_events` → client-link (third stream family; its one consumer bridges to SSE like the frame stream), `replay_events` → **dropped** (zero production consumers; the runtime calls itself to build the subscription backlog). Legacy sessionless `open_view`/`subscribe_view` pair: verify-then-trim at M3 *(M3 verdict: KEPT — production consumer at `POST /v1/views` + `GET /v1/views/{id}/stream`, `api/views.rs`)*. Granularity: `client-link` = **one trait** (10 methods — the prose "11" was an off-by-one vs the §6.4 table, corrected at M3); `runtime-api` = **four traits** (`RuntimeAccountApi` 11, `RuntimeSettingsApi` 3, `RuntimeMailReadApi` 12, `RuntimeMailWriteApi` 15 → 11 after D21) + an umbrella supertrait for `AppState`. Consumer evidence: `posthaste-server` oauth_routes uses exactly one method (`get_account`) → takes `&dyn RuntimeAccountApi` (XVI); finer than four is ceremony (XXI). | I; XVI; XXI | drained |
| D24 | **The far-node link surface moves out of the /v1 platform.** `link_router`/`start_backend` (`server/link.rs`, 261 LOC) borrow `ApiError` + 3 auth helpers from `posthaste-api` (`link.rs:40-41`), so the standalone far-node binary drags the entire /v1 client platform. Cure: the link wire moves to `posthaste-authority-server` (it *is* the far node's wire — locality) with its own minimal error/auth vocabulary; `posthaste-server` remains the composition root that mounts it and still ships both role binaries (D18 unchanged). | XV; XVI; locality | drained |
| D25 | **`posthaste-config` owns the whole config schema story.** `DaemonSettings` resolution (`read_daemon_settings` + env overrides) lives in `posthaste-api/src/config.rs:115-207` and field-reads an `AppToml` returned by `TomlConfigRepository::read_app_toml()` (`repository.rs:102`) — a private-module (voldemort) type leaked across a crate boundary. Move resolution into `posthaste-config`; remove `read_app_toml` from the public surface. | XIV (one schema owner); III; XV | drained |
| D26 | **Delete the never-applied tuning surface**: `DaemonRuntimeTuning` + 5 sub-structs (`config/runtime.rs`, 142 LOC). **Amended 2026-07-02 at execution:** the premise "zero consumers" was structurally wrong — the types are wired into the public TOML schema as `[daemon.runtime.*]` (`DaemonToml.runtime`, `schema/app.rs:319-322`) with round-trip tests; but *behaviorally* right: `to_app_settings` (`app.rs:336`) never reads them — parsed, round-tripped, never applied. Ruled: **full deletion including the schema field and its tests** — config that parses but does nothing silently deceives the operator (XIII), and the unpublished-app license (§9 invariant 1) covers the TOML schema exactly as it covers the wire. Re-add per-value when something actually consumes it. | XX; XXI; XIII (a setting that lies) | drained |
| D27 | **One node-assembly pipeline.** The roots→settings→logging→`RuntimeBuildConfig` preamble and the fail-closed `LinkAuth` construction (including the panic block) are duplicated verbatim across `startup.rs:17-45,112-126`, `startup_backend.rs:25-42,51-65`, and (third copy, remote shape) `posthaste-runtimed/main.rs:21-43`. Extract one shared assembly helper + one `LinkAuth::from_daemon_settings` (fail-closed) used by all three. | XIV | drained |
| D28 | **One query grammar.** `mail_queries/rules/tokenize.rs` (190 LOC) is a second tokenizer for the same grammar as `domain/search/tokenizer.rs`; `rules.rs:36` pre-splits `in:` tokens then re-feeds the remainder to `parse_query` — quoting/negation drift waiting to happen. Unify inside domain-service (an `in:`-resolver hook); **no new crate** — `mail_queries/` is otherwise a thin adapter over `MailService::query_*_by_rule` (the engine already lives in domain-service), single consumer. | XIV; XX (no speculative crate) | drained |
| D29 | **Intra-crate god-file splits ride the M-step rewrites** (no separate churn): `posthaste-runtime/build.rs` (1441 LOC — assembly/config + `RuntimeHandle` with the 52-method impl + shutdown) splits into `assembly`/`handle`/`shutdown` modules at M3 while the impl block is rewritten; `authority-server/backend.rs` (1069 LOC — read facade + command apply + forward-dispatch + event/settlement pub-sub) splits (reads/commands/pub-sub) at M5 while the dispatch match is rewritten. Explicit non-actions: `sessions.rs` (1135), `views.rs` (1000) are large-but-cohesive; `local_backend.rs` is exactly the 49-method trait impl (cured only upstream); `account_repository.rs` "780 LOC" was a false positive (281 code + ~500 tests); `oauth/` stays a module (both consumers live inside the far-node composition — no second out-of-composition consumer, XX); `supervisor/` is cohesive and correctly placed. | I; XXI; XX | drained |
| D30 | **M1 strand ruling (2026-07-02): the provider closure is model-resident.** Executing D10 surfaced that §6.1a's assignment could not compile: `provider/kind_profile.rs` (model, per §6.1a) holds `impl AccountSettings { provider_profile() -> ProviderProfile }`, and Rust's inherent-impl rule pins that impl — and therefore `ProviderProfile` and its field/method closure — to the model crate. Net relocations beyond §6.1a: the whole `provider/` module (6 files: kind_profile + 4 policies + remote_observation), `imap/{capabilities,mailbox_roles}.rs`, and `CacheBudget`+`clamp_unit` (from `cache/scoring.rs`). **Ruled: accept** — all are pure serde data + trivial constructors, the model crate stays wasm-clean (gate-proved), and the alternative (refactoring inherent impls into service-side free fns) is churn against XXI with no boundary gain. `domain-service` retains `imap/{identities,planning}.rs`, cache scoring/governor, search — the *behavioral* logic. §6.1a and topology §1.1–1.2 amended to match. Also from M1: `for_each_link_op!`'s domain-type paths route through link-contract's `$crate::reexport` (existing pattern) — dissolves with the macro's crate at M4/M5. | XXI; XIV; I (the type owns its impls) | drained |
| D31 | **Rename `posthaste-api` → `posthaste-http-api-adapter`** (2026-07-02, user-directed). After M3, "api" means *typed trait surface* everywhere (`posthaste-runtime-api`, `RuntimeApi`); the HTTP platform crate's name said nothing about what it owns and collided head-on. The crate is the HTTP **adapter** over the runtime's typed Api surfaces — name it that, explicitly. Rides M4. | XXII | accepted |
| D32 | **Suffix taxonomy — standardize the seam vocabulary** (2026-07-02, user-directed; supersedes two D17 name choices): **`*Api`** = typed wire-free RPC surface (`RuntimeApi` + its four subtraits); **`*Link`** = replication-link contract (a coherent-link seam: mutation forward + frame subscribe + read-through); **`*Handle`** = owning wrapper (`RuntimeHandle` pattern); **`*Adapter`** = protocol translation (`HttpApiAdapter`). Therefore `BackendApi → AuthorityServerLink` (NOT `AuthorityServerApi` — it is a link, not an RPC surface; calling the link an Api is the same collision D31 kills) and `BackendLink → AuthorityServerLinkHandle` (the wrapper). The two seams stay separate traits (R1 stands — unifying would force a second vocabulary dispatch); what is shared is the `MailOperation` vocabulary and contract-core, not the trait. Assertion ids follow (`one-authority-server-api → one-authority-server-link`). | XXII; XIV (one meaning per suffix) | accepted |
| D33 | **Seam symmetry — split the far-node trait into `AuthorityServerApi` + `AuthorityServerLink`** (2026-07-02, user-directed; supersedes R1, whose premises D8/D21/M2/M3 dissolved — see R1's note). Each seam of the chain gets the same shape: an **`*Api`** (typed request/response surface — what the nearer tier may request: reads, account/settings ops, `apply_mail_operation`) and a **`*Link`** (the coherent-link mechanics: `forward_mutation`, `subscribe(coverage)` → frames, settlement/watermark, outbox op-lifecycle incl. `retry_operation`). Client↔runtime: `RuntimeApi` + `RuntimeLink` (M3's `RuntimeLinkOps` renamed — the taxonomy makes "Ops" noise; applied at the M4 landing). Runtime↔authority-server: `AuthorityServerApi` + `AuthorityServerLink` (this row). One `MailOperation` vocabulary flows through both; each network hop deserializes at its own trust boundary (parse-once *per boundary*, III/IV). Execution: M5b — first deliverable is the per-method assignment table of the current 49-method trait (the §6.4 pattern: bucket rule above, dispositions reported before code), then the split, with `LocalAuthorityServer`/`RemoteAuthorityServer` implementing both. | XXII (symmetry reveals the architecture); III/IV; I | accepted |
| D34 | **One meaning per operation — the `MailOperation` processing model** (2026-07-02, user-directed standardization). Three tiers process the same typed op in three declared senses, and each half is shared exactly where an invariant is shared: **(a) the vocabulary** — `MailOperation` in contract-core, one enum (D8). **(b) the local meaning** — `MailOperation::fold_effect(&self)` in contract-core: the pure projection of an op into link-core's fold vocabulary (`MessageChangeDiff`/fold-state effects); contract-core already depends on link-core, layering holds. The client's optimistic fold and the runtime's outbox fold both consume this ONE projection — today the wasm client and the near node each derive the op's local effect separately, a drift bug waiting to happen (XIV). **(c) the entries** — uniform naming per D32/D33: the request surfaces expose `apply(op: MailOperation) -> CommandAck` (renames D21's `apply_mail_operation`) on `RuntimeMailWriteApi` and `AuthorityServerApi`; the link surfaces carry the op via `run_mutation`/`forward_mutation` (outbox+settlement semantics). **(corrected 2026-07-02, user)**: the client and the runtime are the SAME near-node mechanism at different seams — both accept the typed op, enqueue it in an outbox (`EntityStore` pending / `RuntimeAuthorityServerOutbox`), fold for optimism via (b), flush up-channel, and retire on settlement/base absorption; one implementation, link-core's `MessageReplica` engine (`one-replica-both-seams`; D9/M6 make the seam explicit and lift the retire invariant into the engine). `apply(op)` on the Api surfaces exists for callers that are NOT replicas (no outbox to be optimistic in — the REST command path); only the authority server applies authoritatively (providers). **Rejected within this row:** a shared cross-tier processor trait for the side-effect half — outbox-enqueue and provider-dispatch share a signature but no invariant; unifying them is spooky action (R2's logic). The sharing is vocabulary + fold projection + naming, not implementation. | XIV (one fold, one vocabulary); III; XXII | accepted |
| D35 | **Near-node symmetry gets names** (2026-07-02, user-directed). (a) D9's trait seam is **`OptimisticReplica`** (link-core): the shared near-node mechanism — accept pending, fold-on-read, retire-on-absorption — that `EntityStore` (client) and the runtime's outbox both implement; the wrapper names keep naming their *additional* role (reactive keyed view store; outbox+read-cache overlay), the trait names the shared invariant. Executed at M6. (b) **One up-channel verb**: both Link traits name the flush-pending-mutation-upstream entry `forward_mutation(MutationRequest) -> MutationReceipt` (client-link's `run_mutation` renamed) — the op is *forwarded* because it was already accepted into the local outbox, which is the optimism. Down-channels stay per-seam (frames are emitter-named; D2 stands). A shared supertrait for the up-channel only if the two signatures align without contortion — M5b verifies and records the verdict; same-name convention suffices otherwise (R2: don't share a signature where no invariant is shared). | XXII; XIV | accepted |
| D36 | **Headless-clean near-node layering** (2026-07-02, user-directed; extends D35a). The near-node stack is three layers, each ignorant of the one above: **(1) `OptimisticReplica`** (link-core) — the UI-free, IO-free kernel, sole owner of accept/fold/retire/overlay-on-read semantics (M6 lifts the retire invariant here); **(2) the projection layer** (link-replica) — keyed entities + windowed view rows/predicates over the kernel, still wasm-pure and UI-free (a headless client consumes exactly layers 1+2 + contract-core — this is a REQUIREMENT, not an accident); **(3) tier-specific composition** — client: reactivity + persistence in the TS adapter/wasm binding, never inside the replica crates; runtime: ReadCache/serving composes the SAME layers rather than re-growing its own overlay. M6 scope extension: verify whether the runtime near node's overlay-on-read duplicates layer-2 logic; if yes, the second-consumer rule is met — lift it, don't parallel-implement. P1 (`entity_store.rs` 1800-LOC fusion of mechanism+projection) is the intra-crate split of layers 1/2 inside link-replica — promote it from M9+ candidate into M6's scope since M6 rewrites those exact seams. | I; XIV; XVI (the kernel can't touch what it never sees); XX (headless = the named second consumer) | accepted |
| D37 | **Serving is an adapter, not a replica half** (2026-07-02, user-directed; completes D36's anatomy). Node anatomy is fully compositional: **kernel + projection** (D36 layers 1–2, shared) **+ link-client** (upstream `forward_mutation`+subscribe, every near node) **+ link-server adapter** (downstream serving: sessions, frame collapse/delta, wire pagination — mounted only by nodes that serve; today the runtime's `sessions.rs` and the serving parts of `views.rs`) **+ UI composition** (client only). The projection layer is wire-agnostic — it must not know whether its views render directly or get framed/sessioned/paginated (`ViewSnapshot` is already a contract-core wire type; the adapter owns the framing). Distinction preserved, NOT unified: *authoritative view evaluation* (query→membership over unbounded mail) stays at the authority server per the windowed-view-replica product decision (§4b) — both near nodes share the optimism *projector*, neither owns the *evaluator*. M6 scope note: classify `views.rs` into projection-vs-adapter and move the adapter parts beside `sessions.rs`; the runtime's link-server adapter becomes the one place downstream serving lives. | I; V (the serving path owns the deadline, not the view); XV | accepted |
| D38 | **One projector, two mounts; the evaluator stays behind the Api** (2026-07-02, user-directed; strengthens D36/D37). The view **projector** — base rows + coverage + pending ops → windowed view rows and deltas (recompute-not-invalidate) — is ONE layer-2 component (link-replica) mounted by both near nodes; after D37 removes the serving adapter from the runtime's `views.rs`, the remainder is projector code that merges into layer 2, not a sibling of it. The view **evaluator** — query → membership over the unbounded mailbox, smart-mailbox rules, threading — is authority-server-only (it requires `MailService`/store), reached via `AuthorityServerApi` reads, and structurally cannot leak into the client: the D15 frontier CI proves the client closure contains no evaluator dependency. M6's verify-and-lift upgrades from "check for duplication" to: the end-state is one projector; any piece that genuinely cannot be shared is reported with its reason, not quietly kept as a fork. | XIV (one projector); XVI; I | accepted |
| D39 | **Link-role vocabulary: near-end / far-end** (2026-07-02, user-directed). The two halves of a link are named after the replication canon's node roles: the **link near-end** = the consuming half every near node holds (the Link trait + a transport impl: `forward_mutation` up, `subscribe` down); the **link far-end** = the serving half only fan-in nodes mount (sessions/frame pump/per-link registry/wire routing). Replaces the working names "link-client"/"link-server adapter" (collided with the client/server tiers). **Rejected:** `link-up`/`link-down` — collides with the up-/down-*channels*, which are directions inside the link that both ends carry. Module/type naming at M5b/M6 follows this vocabulary (the runtime's far-end = `sessions.rs` + serving parts of `views.rs`; the authority server's far-end = `link_wire.rs` + registry). | XXII; XIV (reuse the near/far canon) | accepted |
| D40 | **Generic link-end engines** (2026-07-02, user-directed; builds on D39). The two link ends become shared engines instantiated per seam: **`LinkFarEnd<Frame, SubscriberId>`** — subscriber registry, base-broadcast to all + settlement-route to originator (the same rule at both seams, replication L1 §10), cursor replay — instantiated by the runtime (sessions) and the authority server (link registry); **`LinkNearEnd`** — forward POST + frame subscription + the resilience policy (deadlines, jittered reconnect, level-triggered reconciler) — instantiated by the runtime (toward the authority server) and the in-process/desktop client path. Motivation beyond symmetry: the resilience policy is one fact currently forked three ways and broken identically in all three (lifecycle-debt rows 1, 2, 8 — no deadlines, no reconnect, flat retry); one engine = the fix lands once. **Honest limit:** the web client's near-end is TypeScript — it shares the specified policy and generated wire types, not the Rust engine, unless/until the transport moves behind the wasm boundary (recorded as the open follow-up inside this row, not assumed). Execution: **M9** (first next-wave unit; M6's trait seams are its prerequisite). Second consumer named for every seam (XX satisfied by construction). | XIV (one policy, one routing rule); XIX; XX | accepted |
| D41 | **The client's near-end moves behind the wasm boundary** (2026-07-02, user-directed; closes D40's "honest limit"). The web client's transport+resilience layer (today `sessionClient.ts`/`httpAdapter.ts`: reconnect, cursors, frame demux) is replaced by the shared `LinkNearEnd` engine compiled to wasm; TS keeps UI composition and, if raw browser IO requires it, a thin IO shim (fetch/EventSource callbacks) with ZERO policy in it — deadlines/backoff/reconciler/cursor state all live in the engine, once. This retires the TS policy fork (lifecycle-debt row 8) rather than mirroring it. Design choice at M9: engine generic over a `Transport` trait; browser impl binds the shim, native impl binds reqwest/SSE. | XIV; XIX; II (one seam for the policy) | accepted |
| D42 | **One subscriber-id concept: `<Seam>LinkId`** (2026-07-02, user-directed). `RuntimeSessionId → RuntimeLinkId` — a client "session" IS the client's per-connection link instance at the runtime seam (views scoped within it, cursor, settlement routing), exactly as D3 established for the other seam; the id names the link connection, not the party. Family becomes regular: `<Seam>Frame` / `<Seam>Link` / `<Seam>LinkId` / `<Seam>LinkHandle`; the far-end engine is `LinkFarEnd<Frame, LinkId>`. **Rejected:** `SessionId` everywhere — dilutes a UI-connoted word into the replication protocol and re-litigates D3. "Session" as protocol vocabulary (open_session, session-scoped) retires in favor of link-connection terms at M9 when the far-end engine reshapes those APIs; client-app-internal UI naming may keep "session" locally. | XXII; XIV | accepted |
| D43 | **The replica family gets the replica name** (2026-07-02, user-directed). `posthaste-link-core → posthaste-replica-core` and `posthaste-link-replica → posthaste-replica-projector`. Rationale: "link" predates the D32 taxonomy, when it named the whole coherent-link concept; post-D32/D39/D40, *Link* means the seam (contract traits + transports) — and these two crates contain zero transport and zero seam. They are the replica state machine (kernel: `OptimisticReplica`, fold, settlement — nuance: convergence state, NOT persistence, which stays layer-3 per D36) and the projector (D38). Family split becomes: **replica-\*** = state (replica-core, replica-projector); **\*-link** = seams (client-link, authority-server-link, M9's end engines). `posthaste-link-wasm`'s name is decided at M9 with D41 (it becomes the wasm client node assembly: kernel+projector+near-end). Partially supersedes R4 (which rejected renaming link-core on churn grounds — the taxonomy shift changed the calculus: the name now actively lies). Executes at M9's rename wave incl. the CI frontier crate list. | XXII; XIV (one meaning for "link") | accepted |
| D44 | **The reconciler owns outbox replay** (2026-07-02, user-ruled Q1/Q2). `LinkNearEnd`'s level-triggered reconciler runs on EVERY connect (first connect at boot included) and drives: (a) never-dispatched replay (subsumes and DELETES the view-open `resendNeverDispatched` trigger — no secondary trigger remains); (b) sent-but-unsettled reconciliation (receipt held, no terminal settlement — session-continuity loss): query the runtime's pending/settled state by stored `ClientMutationId` and settle or re-forward; the entityStoreAdapter TODO dissolves. | XIV; X | accepted |
| D45 | **Engine homes: two crates named by role, purity-split** (2026-07-02, user-ruled Q3/Q4). `posthaste-link-near-end` — wasm-pure: the `Transport` trait (postJson + stream-open primitives), the near-end engine, resilience policy, reconciler; joins the frontier CI list. `posthaste-link-far-end` — native (tokio): the composable far-end sub-stores (idempotency-dedup store, settlement-sink store, replay store — D40 as amended by Q7: sub-stores each far-end assembles, NOT one struct-per-subscriber). Wasm boundary shape (Q3): JS IO callbacks handed to the engine at construction; engine exposes Promise-returning lifecycle methods. Neither seam crate hosts the engines — they serve both seams. | XXII (D39 vocabulary); XV; XVI (purity enforced by crate boundary) | accepted |
| D46 | **Replay is a strategy parameter** (2026-07-02, user-ruled Q5). The far-end replay store is generic over `ReplayStrategy`: `SeqBacklog` (cursor + bounded buffers — the client seam, existing semantics) and `FullSnapshot` (re-send current base for the covered range on reconnect — the AS seam, new; correct by level-based absorption). Both seams gain reconnect replay at M9; seq-backlog for the AS seam stays available as a later bandwidth optimization, not built now. | XX; XIX | accepted |
| R4  | **Rejected:** renaming `posthaste-link-core` (`-effect-core`/`-mail-effect` were floated). The crate *is* the core of the link mechanism — convergence engine + fold state; the current name reveals that accurately. A rename would churn every consumer for no reveal gain. | XXII (precision, not novelty); XXI; XX | rejected |
| R2  | **Rejected:** splitting the runtime outbox from its replica into two stateless traits over one hidden shared store. No double-storage exists (`MessageReplica` holds base + pending; optimistic state is folded on read). The two roles share **one** invariant ("pending until the base absorbs it"); splitting it is spooky-action — more coupling disguised as less. Expose two **trait views** over the one owner if callers benefit; never duplicate/hide the store. | XIV (spooky-action tension); XXI | rejected |
| R3  | **Rejected:** `posthaste-client-contract` / `posthaste-authority-server-contract` as standalone crates. The client consumes the runtime's contract (no own contract); the backend's contract *is* its link. The type/link matrix is sparse (3 of 6 cells real), not 2×3. | XX (no empty cells) | rejected |

## 3. Name inventory

The find/replace worklist. Filled during Phase-1 traversal; seeded from accepted
decisions.

| Old | New | Kind | Target crate | Status |
|-----|-----|------|--------------|--------|
| `DownFrame` | `AuthorityServerFrame` (was `BackendFrame`; D17 supersedes D1's name) | type (enum) | posthaste-authority-server-link | drained |
| `RuntimeId` | `AuthorityServerLinkId` (was `BackendLinkId`; D17 supersedes D3's name) | type (newtype) | posthaste-authority-server-link | drained |
| `posthaste-link-contract` | `posthaste-authority-server-link` | crate | — | drained |
| `posthaste-authority-runtime` | `posthaste-authority-server` | crate | — | drained |
| `BackendApi` | `AuthorityServerLink` *(D32 supersedes D17's `AuthorityServerApi` — it is a link, not an RPC surface)* | trait | posthaste-authority-server-link | accepted |
| `BackendLink` | `AuthorityServerLinkHandle` *(D32)* | type | posthaste-authority-server-link | accepted |
| `BackendNode` | `AuthorityServerNode` | type | posthaste-authority-server | drained |
| `RemoteBackend` / `LocalBackend` | `RemoteAuthorityServer` / `LocalAuthorityServer` | types | posthaste-runtime / posthaste-authority-server | drained |
| `RuntimeBackendOutbox` | `RuntimeAuthorityServerOutbox` *(may shorten at landing)* | type | posthaste-runtime | drained |
| `docs/backend/` | `docs/authority-server/` | spec dir | — | drained |
| `docs/replication/backend-link/` | `docs/replication/authority-server-link/` | spec dir | — | drained |
| bin `posthaste_backend` | `posthaste-authority-server` | binary | posthaste-server crate | drained |
| bin `posthaste_runtime_daemon` | `posthaste-runtime` | binary | posthaste-runtimed | drained |
| bin `posthaste_daemon` | `posthaste-authority-runtime-server` | binary | posthaste-server crate | drained |
| bin (desktop app) | `posthaste-client` | binary | apps/desktop | drained |
| `posthaste-api` | `posthaste-http-api-adapter` (D31) | crate | — | accepted |
| `RuntimeLinkOps` | `RuntimeLink` (D32/D33 symmetry) | trait | posthaste-client-link | accepted |
| `RuntimeLink::run_mutation` | `forward_mutation` (D35b — one up-channel verb) | trait method | posthaste-client-link | accepted |
| *(new)* `OptimisticReplica` | trait over `MessageReplica` (D35a, names D9's seam) | trait | posthaste-link-core | accepted |
| `mutation.retry` | *(deleted — dead; op retry is `retry_operation`, Q8)* | named mutation | — | drained |
| `posthaste-contract-core` | *(new)* | crate | — | drained |
| `posthaste-runtime-api` | *(new, from `RuntimeCore` split)* | crate | — | drained |
| `posthaste-client-link` | *(new, from `RuntimeCore` split)* | crate | — | drained |
| `MutationRequest { name, args }` | `MailOperation` (typed enum) | type | posthaste-contract-core | drained |
| `MessageMutation` | absorbed into `MailOperation` (D11) | type (enum) | posthaste-contract-core | drained |
| `WireMutationId` | *(deleted — use `link_core::MutationId`, D12)* | type (newtype) | — | drained |
| `posthaste-link-core` | *(no change — rename rejected, R4)* | crate | — | rejected |
| `posthaste-domain` (types slice) | `posthaste-domain-model` | crate | — | drained |
| `posthaste-domain` (logic slice) | `posthaste-domain-service` | crate | — | drained |
| `ConfigError` | *(moves module: `config.rs` → domain-model, D13)* | type (enum) | posthaste-domain-model | drained |
| `ValidationError` | *(moves module: `validation.rs` → domain-model, D13)* | type (enum) | posthaste-domain-model | drained |
| `posthaste_store::*` domain re-exports | *(deleted — import from domain crates, D14)* | re-exports (~80) | posthaste-store | drained |
| `posthaste_authority_runtime::{RuntimeHandle, …}` near-node re-exports | *(deleted — D19a; consumers depend on `posthaste-runtime`)* | re-exports (10) | posthaste-authority-server | drained |
| `posthaste_server::*` api facade + `pub mod oauth`/`supervisor` | *(deleted — D19b)* | re-exports | posthaste-server | drained |
| `MigrationRuntime`, dead `from_api_bridge_*`, provider traits, `migration.rs` | *(deleted / moved to testkit — D20)* | migration residue | posthaste-authority-server, posthaste-server | drained |
| `RuntimeCore::{set_message_keywords, add/remove_message_from_mailbox, replace_message_mailboxes, destroy_message}` | `apply_mail_operation(MailOperation) -> CommandAck` (D21) | trait methods (5→1) | posthaste-runtime-api | drained |
| `revCursor` (string-compared control mutation) | `MailOperation` control variant (D22) | wire name | posthaste-contract-core | drained |
| `RuntimeCore::replay_events` | *(deleted — zero production consumers, D23)* | trait method | — | drained |
| `RuntimeCore::{open_view, subscribe_view}` (sessionless pair) | *(verify-then-trim at M3, D23)* | trait methods | — | drained |
| `link_router` / `start_backend` | *(move: posthaste-server → posthaste-authority-server, own error/auth vocab, D24)* | fns/module | posthaste-authority-server | drained |
| `TomlConfigRepository::read_app_toml` | *(removed from public surface; `DaemonSettings` resolution moves to config, D25)* | method | posthaste-config | drained |
| `DaemonRuntimeTuning` + 5 sub-structs | *(deleted — zero consumers, D26)* | types | posthaste-config | drained |
| `mail_queries/rules/tokenize.rs` | *(deleted — unified into domain search tokenizer, D28)* | module | posthaste-domain-service | drained |

## 4. Open questions (block specific drains)

- **Q1 (RESOLVED 2026-07-01 → D6 accepted):** The up-vocab *is* wasm-pure —
  `RuntimeAdapterError` is `code`/`String`/`bool`/`Value`, ids are `u64`
  newtypes, no `uuid`/`chrono`/`tokio`/`reqwest`. So merging into `link-core` was
  *possible* but **rejected on layering**: it splits the wire vocabulary across
  the domain boundary and strands the `From<ServiceErrorKind>` impl. Up-vocab →
  `contract-core` (above domain). See D6.
- **Q3 (RESOLVED 2026-07-01 → D11 accepted):** `MailOperation` **supersedes**
  `MessageMutation` — the existing enum is already the typed dispatch table, just
  stranded in the backend-link crate; it moves to `contract-core` and becomes the
  canonical carried form of D8.
- **Q4 (RESOLVED 2026-07-02 — yes-drops; per-item evidence §6.5):** `link-wasm`'s
  imports from `link-contract` and `runtime-contract` are exactly one type each —
  `message_mutation::MessageMutation` (`mutation.rs:13`) and `MutationRequest`
  (`mutation.rs:15`). Both land in `contract-core` (D5/D11), so **both** deps
  evaporate at M5; `link-wasm` never imports domain directly (not even model).
  End-state direct deps: `link-core + link-replica + contract-core` (domain-model
  transitive via contract-core — the D15 closure holds). Residual: the
  `Cargo.toml` `uuid = { features = ["js"] }` override exists for the
  wasm32 target while `uuid` is anywhere in the closure; domain-model keeps
  `uuid` (`generated_id`), so the override stays unless that gets feature-gated.
  M5 asserts the dep-drop at landing.
- **Q5 (RESOLVED 2026-07-02 → D18):** binaries are named after their component.
  Inventory: `posthaste-server` ships `posthaste_daemon` (bundled all-in-one) +
  `posthaste_backend` (far node); `posthaste-runtimed` ships
  `posthaste_runtime_daemon`; `apps/desktop` is the client; tools
  (`posthaste-wizard`/`-lab`/`-profile`) unaffected. End-state:
  `posthaste-authority-server`, `posthaste-runtime`, `posthaste-client`,
  `posthaste-authority-runtime-server` (bundled all-in-one).
- **Q6 (RESOLVED 2026-07-02):** the non-replica delta path is removed (user
  confirmation + commit `7b0a7e014`) — *intent already realized*. Trim
  `client-link/L2 §4`'s description of it rather than marker it.
- **Q7 (RESOLVED 2026-07-02):** undo/redo is **authority-server-logged,
  client-navigated**. Evidence: `posthaste-runtime/src/build.rs:568-570` —
  "Undo/redo history is client-owned … the runtime no longer records
  change-diffs or navigates a history stack"; the durable `RevLog`
  (`domain/model/rev_log.rs`, `RevLogStore` ports, store impl) is owned by the
  authority server and the client stack reconciles against it
  (`reconcileWithServer`); undo/redo execute as ordinary `message.applyDiff`.
  No `mutationHistory` frames exist. `mutations/L2 §1`'s runtime-owned
  per-session stack (and `undo-stack-runtime-owned`) is dead text — replace
  with the two-owner statement; the runtime tier owns nothing.
- **Q8 (RESOLVED 2026-07-02):** `mutation.retry` is dead — no code evidence.
  Remove from `mutations/L1 §7`. Distinct and live: `retry_operation`, the
  link op / HTTP endpoint (`POST …/operations/{id}/retry`,
  `link-contract/lib.rs:601,953`) that retries a failed durable **outbox
  operation** — that is op-lifecycle, not a named mutation; keep it where the
  outbox lifecycle is specced.
- **Q9 (RESOLVED 2026-07-02):** wire the four uncatalogued live variants into
  `mutations/L1` semantics rows: `SetKeywords`, `ReplaceMailboxes`, `Snooze`,
  `Unsnooze` (full live enum: setKeywords, setReadState, setFlaggedState,
  setUserTags, moveToMailbox, moveToRole, replaceMailboxes, destroy, applyDiff,
  snooze, unsnooze — `message_mutation.rs:27-39`). Semantics are read from the
  dispatch/fold code, not invented.

## 4b. Settled clarifications (not decisions — corrections to a stale model)

- **The client is a *windowed view replica*, by design.** It holds
  `MessageSummary` projections as opaque `Value` (fold path `link-replica →
  link-core` is domain-free), windowed via `CoverageRange [TOP, k]` +
  continuation/anchors, refreshed by a row firehose. It does **not** hold the
  whole mailbox, nor derive views locally — mail is unbounded (can't ship to a
  browser) and view derivation (smart-mailbox rules, JMAP query, threading,
  pagination) is server-authoritative. Full-local-first (client holds entities +
  derives views) is a *different product architecture*, **out of scope**.
- **The minimal `MessageFoldState` predictor is load-bearing for decoupling.**
  It keeps the fold path domain-free *and* avoids the `domain → link-core →
  domain` cycle (domain calls the predictor in `message_queries`). A
  domain-typed predictor would cycle or force the client to fold a full
  `MessageSummary`. (Corrects a mid-conversation walk-back: the fold path is
  domain-free; only the wasm *binding* pulls domain via `runtime-contract`.)

- **The near node already moved crates; the stale specs never followed.**
  `replication/backend-link/L2` (stale) locates the runtime near node, outbox,
  and `RuntimeCore` impl in `posthaste-authority-runtime`; on disk they live in
  `crates/posthaste-runtime/src/{build,near_node,sessions}.rs` (confirmed by
  issue `L2-runtime-nearnode-remote-seam`, whose item A landed
  `RuntimeBackendOutbox` wrapping a `MessageReplica` there). The revised specs
  state the D7 end-state directly; the drift is recorded here so nobody
  "corrects" a revised spec back to the stale crate name.

## 5. Traversal ledger (Phase 1)

Per-crate completeness tracker. A crate is **done** when every public item has a
row (in §3 or marked "no change"). Order is bottom-up along the dep graph.

| Crate | Traversed? | Notes |
|-------|-----------|-------|
| posthaste-domain | ☑ 2026-07-01 | module-by-module classification + 12-dependent matrix in §6.1; decisions D10/D13/D14 |
| posthaste-link-core | ☑ 2026-07-01 | clean wasm-pure leaf; model example of the split; no change beyond D9 (trait seam) + D12 (its `MutationId` becomes the one wire id) |
| posthaste-runtime-contract | ☑ 2026-07-01 | marooned-types claim confirmed from the consumer side (§6.2); splits per D5/D7; **method-level D7 assignment for all 52 `RuntimeCore` methods: §6.4** (M3 executes from it) |
| posthaste-link-contract | ☑ 2026-07-01 | D1/D3/D4 renames + D11 (`MessageMutation` out) + D12 (`WireMutationId` deleted); trait+frames stay |
| posthaste-link-replica | ☑ 2026-07-01 | wasm-pure types + `EntityStore`; no boundary change |
| posthaste-authority-runtime | ☑ 2026-07-02 | per-item pass done (§6.7): near-node re-export shim killed (D19a); migration-bridge residue sunset (D20); dual command path found → D21; second tokenizer → D28; `backend.rs` split rides M5 (D29); false alarms cleared (`account_repository`, `oauth/`, `supervisor/`, `mail_queries/` verdicts in D29) |
| posthaste-runtime | ☑ 2026-07-02 | per-item pass done (§6.7): `build.rs` split rides M3 (D29); `sessions.rs`/`views.rs` cohesive — no action; `ReadCache` mirrors the trait's read half (shrinks only with the trait); `revCursor` string escape hatch → D22 |
| posthaste-link-wasm | ☑ 2026-07-02 | per-item pass done (§6.5): surface = 2 free fns + `EntityStoreHandle` (14 methods); reaches into `link-contract`/`runtime-contract` are exactly `MessageMutation`/`MutationRequest` → Q4 resolved yes-drops; stale `MailListReplicaHandle` doc-link noted |
| posthaste-server | ☑ 2026-07-02 | per-item pass done (§6.8): clean composition root except the api glob facade + dead `pub mod oauth` (D19b), the far-node link wire borrowing the /v1 platform (D24), triplicated assembly (D27), test-only `migration.rs` in prod surface (D20) |
| posthaste-api / posthaste-config | ☑ 2026-07-02 | per-item pass done (§6.8): api — no file >700 LOC, `AppState` 7 fields, wrapper migration 100% complete (D20), 50 of 52 `RuntimeCore` methods dispatched at 63 sites (confirms D7/D23); config — clean adapter post-D10, but owns `DaemonSettings` resolution by proxy (D25) and ships a dead tuning surface (D26) |

## 6. Evidence appendix (2026-07-01 traversal)

Ground truth backing the decision rows. Line numbers are as of this date; treat
as approximate after any code motion.

### 6.1 `posthaste-domain` split (D10/D13)

**(a) Module → slice assignment.** The internal seam already exists; the split
is a crate boundary, not a detangle.

`posthaste-domain-model` (deps: `serde`, `serde_json`, `thiserror`, `time`,
`uuid`, `futures_util`, `utoipa` opt + the `openapi` feature):

- `model/*` (all 15 files — ids via `string_id!`, messages, records, commands,
  errors, outbox, sync, rev_log, smart_mailboxes, conversations, automation,
  appearance, notifications, account_settings, account_overview)
- `vocab.rs` (`MailboxRole`, `SystemKeyword`), `generated_id.rs` (`Id`, sole
  `uuid` user)
- `cache/primitives.rs`, `cache/entities.rs`, `cache/resource_types.rs`,
  plus `CacheBudget`/`clamp_unit` (D30 strand)
- `imap/types.rs`, `imap/sync_state.rs`, plus `imap/{capabilities,
  mailbox_roles}.rs` (D30 strand)
- the whole `provider/` module — `kind_profile` + the 4 policy files +
  `remote_observation` (D30 strand: `ProviderProfile`'s inherent-impl closure)
- relocated enums: `ConfigError` (from `config.rs`), `ValidationError` + its
  `From` impls (from `validation.rs`) — D13

`posthaste-domain-service` (deps: model + `async-trait`, `mail-parser`,
`posthaste-observability`, `posthaste-link-core`; forwards
`openapi = ["posthaste-domain-model/openapi"]`):

- `service/*` (MailService + 10 submodules), `ports/*` (~30 port traits incl.
  the `MailStore` 22-subtrait composite)
- `config.rs` minus `ConfigError` (keeps `ConfigRepository`, `ConfigSnapshot`,
  `ConfigDiff`), `validation.rs` minus the enum (keeps `validate_*` fns)
- `secret.rs` (`SecretResolver`), `push.rs` (`PushTransport` + stream types)
- `cache/{scoring,resource_governor}.rs`, `imap/{capabilities,identities,
  mailbox_roles,planning}.rs`, `provider/*_policy.rs` +
  `remote_observation.rs`, `search/*`

**(b) Dependent classification (12 workspace dependents).** Model-only after
the split: `posthaste-link-contract` (pure types today), `posthaste-runtime-contract`
(pure types once D13 relocates the error/validation enums). Service-tier (need
traits/impls): `runtime` (`SecretStore`), `config` (`ConfigRepository` +
`validate_snapshot`), `store` (implements every store port), `engine`
(`MailGateway`/`PushTransport`), `imap` (gateway + imap planning fns), `api` /
`server` / `authority-runtime` / `testkit` / `bench` (`MailService` + stores).

**(c) Strands that forced D13 and the extra model files:**

- `ServiceError::Config(#[from] crate::ConfigError)` (`model/errors.rs:64`) —
  `ServiceError` can't be model-pure unless `ConfigError` comes along.
- `impl From<ValidationError> for ConfigError` (`config.rs:44,50`) — the two
  enums must be co-located (orphan rule); the `validate_*` *functions* stay in
  service.
- `AppSettings.cache_policy: CachePolicy`, `AccountDriverCapabilities.cache_fetch_unit`
  (`model/account_settings.rs`) — pulls `cache/primitives.rs` into model.
- `SyncBatch` holds `Vec<ImapMailboxSyncState>` etc. (`model/records.rs:126-133`)
  — pulls `imap/{types,sync_state}.rs` into model.
- `ProviderHint: From<ProviderKind>` (`model/account_settings.rs:201`) — pulls
  `provider/kind_profile.rs` into model.

**(d) Flat-namespace caveat.** `lib.rs` glob-re-exports every module, and
dependents import `posthaste_domain::X` flatly. End-state (D10): no glob shim in
`domain-service`; the 10 service-tier crates re-point imports (mechanical), the
2 contract crates re-point to `domain-model`.

### 6.2 Crate graph & marooned wire types (D5/D7/D11/D12/D15)

**(a) Today's layering** (posthaste path-deps only):
`{observability, link-core}` → `{link-replica, domain}` →
`{config, store, engine, imap, runtime-contract}` → `link-contract` →
`{link-wasm, runtime}` → `{authority-runtime, api}` → `{server, runtimed}`.

**(b) `runtime-contract` fuses two things:** ~40 wire types (ids `lib.rs:68-76`,
`MutationRequest` `:730`, `MutationReceipt` `:742`, `MutationSettlementState`
`:783`, `RuntimeFrame` `:371`, `ViewFrame` `:465`, `ViewSnapshot` `:683`,
`MailListViewState` `:591`, `RuntimeAdapterError` `:804`, `mutation_args::*`,
`mail_query::*`) and the 52-method `RuntimeCore` god-trait (`lib.rs:1011-1354`; counted per-item 2026-07-02, §6.4).
Trait consumers: `api`, `server` (oauth), harnesses. Type-only consumers:
everyone else. → D5/D7.

**(c) `link-contract` repeats the fusion** at the backend link: wire types
(`DownFrame` `lib.rs:130`, `BaseAssertion`, `BaseUpdate`, `RuntimeId` `:181`,
`WireMutationId` `:147`, `LinkCoverage`, `LINK_*_PATH`, generated request
structs) + `BackendApi` (~50 default methods) + `BackendLink` + the pure
`MessageMutation` enum (`message_mutation.rs:27`). Cure: D11 (move the shared
types out), not a second crate split (R3 holds).

**(d) Wrong edges vs the intended hierarchy** (`links → contract-core →
domain-model`):

1. `runtime-contract → domain` (full god-crate) — *the* edge dragging
   `mail-parser`+`tokio` into the wasm path (`link-wasm → runtime-contract →
   domain`). Fix: D10 + re-point.
2. `link-contract → runtime-contract` (whole crate incl. `RuntimeCore`) for
   types it imports at `lib.rs:45-51`. Fix: D5 re-point to `contract-core`.
3. `config → domain` for schema/model types only. Fix: D10 re-point (minor).
4. No cycles; the fold path stays domain-free (§4b holds).

**(e) wasm-purity status:** clean today: `link-core`, `link-replica`. Breached
transitively: everything through `runtime-contract`. Native-dep crates that must
stay right of the frontier: `store` (`rusqlite`), `engine`
(`reqwest`/`axum`/`jmap-client`), `imap`, `domain-service` (`mail-parser`,
`tokio` via service deps). → D15.

### 6.3 Related evidence

- Principle audits of 2026-07-01 (git `e5eae6229`, `var/audit-{runtime,backend,client}.md`,
  since removed from the tree): grade the *runtime-lifecycle* debt (no deadline
  on `RemoteBackend` reqwest client, no down-channel reconciler, stub shutdown,
  jitterless push backoff, unbounded domain outbox retry). Orthogonal to crate
  topology — none of it blocks this RFC, but the spec drain (Phase 3) should
  carry the affected sections as `[::state partial]` so the lifecycle fixes have
  a home. The audits also independently flag the god-surfaces this RFC splits
  (`runtime-contract/lib.rs` 1717 LOC, `link-contract` 1353 LOC).

### 6.4 `RuntimeCore` method-level split (D7 — the M3 worklist; 2026-07-02 per-item pass)

All 52 trait methods (`runtime-contract/src/lib.rs:1013-1353`), assigned. Every
method takes `caller: RuntimeCaller` first — omitted below. Buckets: **API** =
`posthaste-runtime-api` (41), **LINK** = `posthaste-client-link` (9), **⚠** =
problem, disposition below (2).

| Method | Shape (params → return) | Bucket | Group |
|---|---|---|---|
| `list_accounts` | () → `RuntimeAccountList` | API | accounts |
| `get_account` | `AccountId` → `AccountOverview` | API | accounts |
| `resolve_account_scope` | `AccountScopeRequest` → `Vec<AccountId>` | API | accounts |
| `create_account` | `CreateAccountMutation` → `AccountOverview` | API | accounts |
| `patch_account` | `AccountId, PatchAccountMutation` → `AccountOverview` | API | accounts |
| `delete_account` | `AccountId` → () | API | accounts |
| `verify_account` | `AccountId` → `AccountVerificationResult` | API | accounts |
| `set_account_enabled` | `AccountId, bool` → () | API | accounts |
| `reload_config` | () → () | API | accounts *(admin-flavored; rides with `set_account_enabled` in its one consumer, `api/accounts/lifecycle.rs`)* |
| `runtime_status` | () → `RuntimeStatus` | API | admin *(test-only consumer today: server/authority-runtime test harnesses; keep — health RPC)* |
| `sync_account` | `AccountId, SyncMode` → `usize` | API | admin |
| `get_app_settings` | () → `AppSettings` | API | settings |
| `patch_app_settings` | `PatchAppSettingsMutation` → `AppSettings` | API | settings |
| `preview_automation_rule` | `AutomationRulePreviewMutation` → `AutomationRulePreviewResult` | API | settings |
| `list_mailboxes` | `AccountScopeRequest` → `BTreeMap<AccountId, Vec<MailboxSummary>>` | API | mail-catalog |
| `set_mailbox_role` | `AccountId, MailboxId, Option<String>` → `Vec<MailboxSummary>` | API | mail-catalog |
| `list_smart_mailboxes` | () → `Vec<SmartMailboxSummary>` | API | mail-catalog |
| `get_smart_mailbox` | `SmartMailboxId` → `SmartMailbox` | API | mail-catalog |
| `create_smart_mailbox` | `CreateSmartMailboxMutation` → `SmartMailbox` | API | mail-catalog |
| `patch_smart_mailbox` | `SmartMailboxId, PatchSmartMailboxMutation` → `SmartMailbox` | API | mail-catalog |
| `delete_smart_mailbox` | `SmartMailboxId` → () | API | mail-catalog |
| `reset_default_smart_mailboxes` | () → `Vec<SmartMailboxSummary>` | API | mail-catalog |
| `list_tags` | `AccountScopeRequest` → `Vec<TagSummary>` | API | mail-catalog |
| `query_mail_page` | `MailQueryRequest` → `MailQueryPage` | API | mail-read |
| `get_message_detail` | `AccountId, MessageId` → `CommandResult` | API | mail-read |
| `get_message_resource` | `AccountId, MessageId, MessageResourceKind` → `RuntimeResourceBytes` | API | mail-read *(D7 prose said "resource streams" — reality is one-shot serde bytes, no stream; it is a plain read RPC)* |
| `get_identity` | `AccountId` → `Identity` | API | compose-outbox |
| `list_sender_addresses` | () → `Vec<CachedSenderAddress>` | API | compose-outbox |
| `get_reply_context` | `AccountId, MessageId` → `ReplyContext` | API | compose-outbox |
| `get_draft_content` | `AccountId, MessageId` → `DraftContent` | API | compose-outbox |
| `send_message` | `AccountId, SendMessageRequest` → () | API | compose-outbox |
| `save_draft` | `AccountId, Option<MessageId>, SendMessageRequest` → `Operation` | API | compose-outbox |
| `delete_draft` | `AccountId, MessageId` → `Operation` | API | compose-outbox |
| `list_pending_operations` | `AccountId` → `Vec<Operation>` | API | compose-outbox |
| `discard_operation` | `AccountId, OperationId` → () | API | compose-outbox |
| `retry_operation` | `AccountId, OperationId` → () | API | compose-outbox |
| `set_message_keywords` | `AccountId, MessageId, SetKeywordsCommand` → `CommandAck` | API | message-commands |
| `add_message_to_mailbox` | `AccountId, MessageId, AddToMailboxCommand` → `CommandAck` | API | message-commands |
| `remove_message_from_mailbox` | `AccountId, MessageId, RemoveFromMailboxCommand` → `CommandAck` | API | message-commands |
| `replace_message_mailboxes` | `AccountId, MessageId, ReplaceMailboxesCommand` → `CommandAck` | API | message-commands |
| `destroy_message` | `AccountId, MessageId` → `CommandAck` | API | message-commands |
| `open_session` | () → `RuntimeSession` | LINK | sessions |
| `close_session` | `RuntimeSessionId` → () | LINK | sessions |
| `subscribe_runtime_frames` | `RuntimeSessionId, Option<RuntimeSessionSeq>` → `RuntimeFrameSubscription` *(catch_up + BoxStream)* | LINK | sessions |
| `open_session_view` | `RuntimeSessionId, ViewDescriptor` → `ViewSnapshot` | LINK | session-views |
| `close_session_view` | `RuntimeSessionId, ViewId` → () | LINK | session-views |
| `extend_session_view` | `RuntimeSessionId, ViewId, usize` → `ViewSnapshot` | LINK | session-views |
| `run_mutation` | `MutationRequest` → `MutationReceipt` | LINK | mutation-forward |
| `open_view` | `ViewDescriptor` → `ViewSnapshot` | LINK | sessionless-views |
| `subscribe_view` | `ViewId, Option<ViewRevision>` → `RuntimeViewSubscription` *(BoxStream)* | LINK | sessionless-views |
| `subscribe_events` | `EventFilter` → `RuntimeEventSubscription` *(replay + BoxStream\<DomainEvent\>)* | ⚠ | events |
| `replay_events` | `EventFilter` → `Vec<DomainEvent>` | ⚠ | events |

**Problem dispositions (the 2 ⚠ rows):**

- `subscribe_events` — mixes the buckets: domain-typed payload (`DomainEvent`,
  runtime-api flavor) but subscription/stream shape (client-link flavor).
  **Disposition: → `posthaste-client-link`, as the third stream family
  (sessions / views / events).** Its one production consumer
  (`api/sync_events.rs:122`, the `/v1` SSE bridge) treats it exactly like the
  frame stream: subscribe → bridge to SSE. A third crate for one method is XX.
- `replay_events` — serde shape says runtime-api, but it has **zero production
  consumers**: only the runtime calling itself (`runtime/build.rs:1166`, building
  the subscription's replay backlog) and one authority-runtime test
  (`authority_runtime_handle.rs:1911`). `RuntimeEventSubscription.replay`
  already carries the backlog. **Disposition: drop from the public trait at M3
  (demote to a `posthaste-runtime` internal); if a consumer materializes it
  belongs beside `subscribe_events`.** (XXI/XX — same reasoning as D16's dead
  settlement arms.)

**Consumer × group matrix** (call-site grep, 2026-07-02):

| Consumer | Groups used |
|---|---|
| `posthaste-api` handlers | **all** — accounts+admin (`api/accounts/*`, `read_calls`, `support`, `sync_events`), settings (`api/settings.rs` only), mail-catalog (`mailboxes.rs`, `smart_mailboxes/crud.rs`, `read_calls.rs`), mail-read (`messages/listing`, `messages/detail*`, `smart_mailboxes/listings`), compose-outbox (`messages/compose/*` only), message-commands (`message_commands.rs` only), sessions/views/mutation-forward (`runtime_stream/*` only), sessionless-views (`views.rs`), events (`sync_events.rs`) |
| `posthaste-server` `oauth_routes` | **accounts only** — exactly one method, `get_account` (`oauth_routes/handlers.rs:93`). (Its other uses, `link.rs`/`startup_backend.rs`, are the authority-server-link seam / node methods, not `RuntimeCore`.) |
| `posthaste-testkit` | accounts+admin setup (`create_account`, `sync_account`, `list_mailboxes`) + link ops in the `settle` helper (`open_session`, `open_session_view`, `subscribe_runtime_frames`, `run_mutation`, `open_view`) |
| `posthaste-bench` | accounts+admin setup (`create_account`, `sync_account`) + link ops (`open_session`, `open_session_view`, `extend_session_view`, `subscribe_runtime_frames`) + `get_message_detail` |

**Trait granularity (recommendation for M3):**

- **`posthaste-client-link`: one trait** (11 methods after `subscribe_events`
  moves in and `replay_events` is dropped). Its four families are one protocol —
  every consumer (api `runtime_stream`+`views`+`sync_events`, testkit `settle`,
  bench workload) opens a session, opens views, subscribes, and forwards
  mutations *together*; splitting one protocol across traits is ceremony with no
  subset consumer (XXI). *(Sub-note: `open_view`/`subscribe_view` are the
  sessionless legacy pair next to the session-scoped path — a candidate D16-style
  verify-then-trim at M3, but that is a separate decision; assign, don't delete.)*
- **`posthaste-runtime-api`: four narrow traits, not one** — a single 41-method
  trait re-creates the B3 god-surface D7 exists to cure. Cut along change-cadence
  (tenet I) where a subset consumer exists (XVI): `RuntimeAccountApi` (accounts +
  admin, 11 — exactly the oauth_routes + harness-setup surface),
  `RuntimeSettingsApi` (3), `RuntimeMailReadApi` (mail-catalog + mail-read, 12),
  `RuntimeMailWriteApi` (compose-outbox + message-commands, 15). Finer than four
  is ceremony: no consumer distinguishes catalog from page-reads or compose from
  commands (the api crate uses all of them; nothing else uses any). An umbrella
  `RuntimeApi: RuntimeAccountApi + RuntimeSettingsApi + RuntimeMailReadApi +
  RuntimeMailWriteApi` (blanket impl) keeps `AppState` at one object;
  `oauth_routes` takes `&dyn RuntimeAccountApi` (XVI).
- Implementor impact: exactly one production impl, `RuntimeHandle`
  (`runtime/build.rs:643`), plus server-test wrappers — five `impl` blocks
  instead of one, same bodies.

### 6.5 `posthaste-link-wasm` per-item surface (Q4; 2026-07-02 pass)

**Public surface (wasm_bindgen exports):**

- `mutation.rs`: `parseMessageMutation(request_json, role_map_json) →
  Option<String>` (mutation → `{messageId, assertion}` for local optimism);
  `invertMessageChangeDiff(diff_json) → String`.
- `entity_store.rs`: `EntityStoreHandle` — constructor + 14 methods:
  `registerViewJson`, `setViewRowsJson`, `closeView`, `ingestBatchJson`,
  `acceptMutationJson`, `settle`, `hasPending`, `messageJson`,
  `captureMutationDiffJson`, `mailboxJson`, `viewRowsJson`, `projectViewJson`,
  `drainDirtyJson`, `drainRetiredJson`.
- Doc rot: `entity_store.rs:5` links `crate::MailListReplicaHandle`, which no
  longer exists — fix in passing at M5.

**Actual imports per dependency** (complete list):

| Dep | Items imported | Post-M2/M5 home |
|---|---|---|
| `link-core` | `MessageAssertion`, `MessageChangeDiff`, `apply_message_assertion`, `MessageOutcome`, `MutationId`, `SettlementOutcome` | stays (frontier crate) |
| `link-replica` | `EntityStore`, `fold_state_from_projection`, `SortDirection`, `SortKey`, `StoreUpdate`, `ViewPredicate`, `ViewRow` | stays (frontier crate) |
| `link-contract` | **only** `message_mutation::MessageMutation` (`mutation.rs:13`) | `contract-core` (D11 → `MailOperation`) — **dep drops** |
| `runtime-contract` | **only** `MutationRequest` (`mutation.rs:15`) | `contract-core` (D5) — **dep drops** |
| domain | *(none — no direct import, not even model types)* | — |

Q4 verdict + the uuid-override residual: see §4 Q4 (resolved yes-drops).

### 6.6 `MutationRequest` wire-crossing map (D8/D11/M5 — the parse-once boundaries; 2026-07-02 pass)

Current shape: `MutationRequest { session_id: Option<RuntimeSessionId>, name:
String, args: Value, client_mutation_id: ClientMutationId, context:
Option<Value> }` (`runtime-contract/lib.rs:730`).

**Builders (who constructs one):**

1. **Web client (TS)** — `apps/web/src/runtime/mutations.ts` (4
   `runtimeSessionClient.runMutation` sites) builds `{name, args,
   clientMutationId}`; `entityStoreAdapter.ts:499` hands the same request to
   `parseMessageMutation` (wasm) for optimism before POSTing.
2. **Nobody else in production.** The runtime *forwards the same value* it
   received (`build.rs` → `backend_link.forward_mutation(request.clone())`) —
   no re-construction on the path. All other construction sites are tests
   (testkit `view_settlement`/`rev_log_append`, server `backend_link_*`,
   authority-runtime handle tests, runtime unit tests, link-contract tests).

**Wire crossings (serialize/parse points) — where the typed `MailOperation`
must be (de)serialized post-M5, and today's redundant parses they replace:**

| # | Crossing | Today | Post-M5 |
|---|---|---|---|
| W1 | client → API: `POST /v1/runtime/sessions/{id}/mutations` | `Json<MutationRequest>` extractor (`api/runtime_stream/mutations.rs:28`) parses the envelope; `name` stays a string inward | serde parse **is** the op parse (typed enum in the derive); reject-unknown at the edge (III) |
| W1b | client-local wasm boundary (same JSON, optimism path) | `link-wasm/mutation.rs:30` parses envelope, then `MessageMutation::from_request` string-dispatch (`:36`) | one serde parse to `MailOperation`, exhaustive match |
| W2 | runtime → authority server (remote): `POST LINK_FORWARD_MUTATION_PATH` | `RemoteBackend` serializes (`runtime/transport.rs:138`) → `server/link.rs:114` `Json<MutationRequest>` parse | same two points, typed payload |
| W2′ | runtime → authority server (colocated) | `LocalBackend::forward_mutation` (`local_backend.rs:115`) — typed pass-through, **no serialization** | unchanged (this is the parse-once payoff: the value stays typed across the in-process hop) |

**In-process string-parses that collapse into the W-crossings** (today's
"three parses of one request" + two arg re-parses):

- runtime near node, optimism fold: `near_node.rs:121`
  `named_message_assertion` → `MessageMutation::from_request`.
- runtime dispatch: `build.rs:561` `if request.name == "revCursor"` routing +
  `dispatch_rev_cursor` re-parsing `request.args` (`build.rs:620-625`).
- runtime session idempotency: `sessions.rs:387` compares `mutation.name` +
  `mutation.args` for re-accept — becomes typed-op `PartialEq`.
- far node dispatch: `backend.rs:934` (`revCursor` string check) + `:937`
  `MessageMutation::from_request`; `apply_rev_cursor` re-parses args again
  (`backend.rs:429-431`).
- far node dedup/receipt: `Backend::forward_mutation_for` keys acceptance on
  `(client_mutation_id, name)` and `MutationReceipt.name: String` echoes it —
  post-M5 the canonical name derives from the op variant (one fact).

**M5 executability findings:**

- **`MailOperation` must cover `revCursor`.** The live vocabulary is the 11
  `message.*` ops (`message_mutation.rs:27-39`, Q9) **plus** the `revCursor`
  control mutation, which lives *outside* `MessageMutation` today (routed by
  string compare at `build.rs:561` / `backend.rs:934`, args `RevCursorArgs`).
  Either a `MailOperation::RevCursor(RevCursorArgs)` arm or a two-arm request
  payload (`Message(MailOperation) | Control(RevCursor)`) — decide at M5; the
  single-enum arm is simpler and keeps dispatch one exhaustive match (XXI).
- `MutationRequest.context: Option<Value>` rides along untyped — it is not part
  of the op vocabulary; leave as-is (XX).

### 6.7 Node-tier per-item pass (2026-07-02): authority-runtime + runtime

**posthaste-authority-runtime** (~9.0k src LOC; public surface = `lib.rs`
pub-uses + `pub mod oauth` + `pub mod supervisor`, everything else
`pub(crate)`): `backend.rs` 1069 (god-file → D29/M5 split), `local_backend.rs`
577 (= the 49-method `AuthorityServerApi` impl; size is the trait's, cured
upstream only), `account_repository.rs` 281 code + ~500 tests (not a god-file),
`build.rs` 510 (assembly fine; dead migration surface → D20), `mutations/` 984
(cohesive `AccountMutationService`), `mail_queries/`+`rules/` 593 (thin adapter
over `MailService::query_*_by_rule`; **not** an embedded engine; second
tokenizer → D28), `oauth/` ~886+tests (cohesive; stays a module — both
consumers live in the far-node composition), `supervisor/` ~2,030 (cohesive
account-lifecycle engine, 13-method facade, correctly placed),
`runtime_registry.rs` 346 (per-tier dedup mirror, documented intentional).
Re-export shim analysis → D19a. Dual command path → D21.

**posthaste-runtime** (~5.0k src LOC; 15 public items): `build.rs` 1441
(three fused concerns → D29/M3 split), `sessions.rs` 1135 + `views.rs` 1000
(large-but-cohesive, survive M3, no action), `read.rs` 599 (`ReadCache` ~25
methods — deliberate 1:1 mirror of the trait's read half), `near_node.rs` 452
(D9/M6 territory), `transport.rs` 348, `secret.rs` 63. Trait counts:
`RuntimeCore` **52** (sole impl `build.rs:643`), `AuthorityServerApi` **49**
(~45 with `Err(unsupported)` defaults; `LocalAuthorityServer` overrides all 49),
everything else ≤3 methods.

### 6.8 Platform-tier per-item pass (2026-07-02): api + server + config

**posthaste-api** (10,469 LOC incl. tests): no file >700 LOC anywhere;
`AppState` = 7 fields (`runtime: RuntimeHandle` + 6 auth/perimeter);
handlers dispatch 50 of 52 `RuntimeCore` methods at 63 sites across 21 files —
api uses essentially the whole trait (confirms D7; runtime-api stays a wide RPC
surface by design, D23). **Wrapper migration 100% complete**: zero
`MailService`/`DatabaseStore`/`posthaste_store` references in `src/`
(`posthaste-store` is dev-dep-only); residue → D20. `authz` route table is
fail-closed with an OpenAPI completeness test (XXIV-guarded, no action).
Serving toolkit (`serve`/`router`/`spa`/`app_state`) intentionally hosts lean
daemons; worst piece (config resolution) → D25.

**posthaste-server** (1,713 src + 7,698 test LOC): clean composition root — no
domain logic found; one runtime dispatch (`oauth_routes/handlers.rs`,
`get_account`). Issues: api glob facade + dead `pub mod oauth` + tests-only
`pub mod supervisor` (D19b), far-node link wire borrowing `ApiError` + 3 auth
helpers from the /v1 platform (`link.rs:40-41` → D24), assembly triplication
incl. the fail-closed `LinkAuth` panic block (`startup.rs:17-45,112-126` ≡
`startup_backend.rs:25-42,51-65` ≡ `runtimed/main.rs:21-43` → D27),
test-only `migration.rs` in the prod surface (D20).

**posthaste-config** (3,119 LOC): clean adapter, D10-conformant — domain
reaches are ~40 pure model types + `ConfigError` + 4 model helpers (all land in
domain-model) and exactly `ConfigRepository`/`ConfigSnapshot`/`ConfigDiff` +
`validate_snapshot` from the service tier (matches §6.1b). Two flags:
`read_app_toml()` leaks a voldemort type to api (D25); `DaemonRuntimeTuning`
+ 5 sub-structs are dead (D26).

### 6.9 `AuthorityServerLink` method assignment (D33 — the M5b worklist; 2026-07-02 pre-pass)

49 methods (post-M4 names, pre-M5 apply-collapse). Ruling applied over the
draft table: **all reads are Api** — D33's "op-lifecycle" rule covers the
lifecycle *mutations* only (`list_pending_operations` re-bucketed Api; it is a
side-effect-free, ReadCache-mirrored query).

**LINK (6):** `forward_mutation`, `forward_mutation_for`, `subscribe`,
`subscribe_for`, `retry_operation`, `discard_operation`.

**API (43):** everything else —
- reads (21, all ReadCache-mirrored 1:1, the Api-half evidence): mail queries/
  detail/conversation/resources, account/settings/catalog/compose reads,
  `replay_events` (has a real prod consumer here, unlike RuntimeCore's — keep),
  `get_draft_content` (Api-shaped; its lazy-fetch side effect rides the
  existing down-channel, not a new entry), `list_pending_operations`,
  `rev_log_snapshot`, `account_count`;
- message commands (5) — fold into `apply(op: MailOperation) -> CommandAck`
  (D34) during the split;
- compose-outbox creation (`send_message`, `save_draft`, `delete_draft`),
  catalog/settings/smart-mailbox/account ops, `sync_account`, `reload_config`.

**D35b verdict (recorded per D35):** the two seams' `forward_mutation`
signatures DIFFER-BECAUSE-CALLER-ARITY — `RuntimeLink` threads per-call
`RuntimeCaller` (one runtime multiplexes many client sessions);
`AuthorityServerLink` scopes identity per-connection (`*_for` variants for
fan-in). A shared supertrait would force one shape onto both — contortion.
**Same-name convention stands; no supertrait.**

## 7. Drain map (Phase 3 worklist)

Revised specs land at the same relative paths the stale tree mirrors
(`docs/stale/X/… → docs/X/…`). *(2026-07-02, D17: after the drain below ran,
the durable dirs `backend/` and `replication/backend-link/` were renamed to
`authority-server/` and `replication/authority-server-link/`; rows below keep
the July-1 names as executed history.)* Per spec: stale claims → the decision that
invalidates each; everything else carries forward. Recipe per file: copy stale
→ apply the decision/name map → correct the near-node crate drift (§4b) → add
`[::state partial plan=eph/RFC-L2-architecture-cleanup]` on sections whose code
lags → bump `modified`/`reviewed`. File-level `state:` stays as-is (`realized`
mechanisms stay `realized`); the inline `[::state]` markers carry the lag — only
the net-new topology spec is `state: planned`.

| Spec (stale → durable) | Stale claims to rewrite | Carries forward (unchanged) |
|---|---|---|
| `replication/L1` | `RuntimeId` (§5.2, §10 → D3); `DownFrame::Base/Settlement` (§10 → D1) | the whole mechanism vocabulary: `link-two-channels`, `view-is-pure-fold`, `recompute-not-invalidate`, `retire-on-confirmation`, `single-local-effect`, `corrections-as-base-updates`, `link-permits-fan-in`; keep §10's existing `[::state partial]` |
| `replication/backend-link/L1+L2+L3` | densest file: crate name (D4), `DownFrame` (D1), `RuntimeId` (D3), `MutationRequest` payload (D8), near-node crate locations (§4b drift, D7), crate-doc overclaim (D4) | `backend-link-is-replication-only`, `one-backend-api` (R1), `one-replica-both-seams` (R2), `transport-selected-by-config`, `colocated-unchanged`, `backend-builds-standalone`; L1 §3.1 keeps its `[::state partial]` |
| `replication/client-link/L1+L2+L3` | L2 §1 "link-core holds the named-mutation vocabulary" (→ D6/D8: up-vocab in contract-core; link-core stays the effect-fold leaf); `posthaste-domain` refs (D10); predictor-pulls-domain nuance restated per §4b | `client-near-node`, `runtime-adapter-opaque`, `predictor-single-crate`, `predictor-wasm-clean`, `replica-rebase-only`, `replica-working-set`, `wasm-boundary-json`, `delta-opt-in-additive`, `settlement-realizes-watermark`; `RuntimeFrame` naming (already emitter-named) |
| `runtime/L1` | `RuntimeId` (§6 → D3); "replica runtime" future crate (→ D7 naming: runtime-api + client-link) | `ui-renderer-only`, `runtime-contract-shared`, `renderer-no-outbox`, `runtime-transport-neutral`, `deployment-transport-only` |
| `runtime/adapter/L1+L2+L3` | `runMutation(name, args)` stringly (D8); `RuntimeAdapterError` home (D6 → contract-core); settlement-state list (D16 trim) | `RuntimeFrame` envelope, opaque-id discipline, `settlement-frame-scope`, single-frame-stream cursor, `api-is-runtime-adapter`/`tauri-is-runtime-adapter` |
| `runtime/internals/L1+L2+L3` | the two-crate picture (§1) → ≥four crates (D5/D7); `posthaste-replica-runtime` name (D7); stringly carried op (D8) | `runtime-contract-implementation-neutral`, `runtime-handle-transport-neutral`, `runtime-handle-no-transport-types`, roots/secret-store ownership, `runtime-builder-transport-free`, `runtime-shutdown-handle`, `multi-window-shared-runtime` |
| `runtime/mutations/L1+L2` | stringly names incl. retired sugar mutations (D8, D16) | mutation semantics per op |
| `backend/L1+L2+L3` | L2 §1.1 crate table: domain god-crate (D10), omits the whole link stack + newer crates (rebuild from §6.2a end-state); `RuntimeCore` refs (D7); `RuntimeId` watermark wording (D1/D3) | hexagonal ports architecture (D10 keeps it), `domain-is-shared-language` (retarget → domain-model), event/state-assertion rules (`event-post-state`, `no-query-invalidation-events`, `confirmation-watermark-emitted`) |
| `client/L1+L2+L3` | L2 §4.1 hook list still names retired sugar mutations (D16) | essentially everything: `renderer-no-outbox`, `renderer-single-optimism`, `single-runtime-adapter`, `adapter-types-single-source` |
| `state/mail/L1+L2+L3` | only by-reference inherits of D1/D3 in §7.1; note `projection`+`countDeltas` (issue `L2-projectionless-sync-events`, done) in L2 §7 | highest-confidence carry-forward: canonical/derived taxonomy, `conversations-derived`, `message-change-before-after`, `message-content-tokens`, query-scope/evaluator model |
| `api/L1+L2+L3+endpoints` | up-channel op type refs (D8); near-node routing mentions (D3) | projection/SSE/pagination/auth contract, `shared-projection-model`, `confirmation-watermark`, `snapshot-recovery` |
| `testing/L0+L1`, `ui/L0+L1` | rename sweep only (§3 map) | everything else |
| *(new)* `architecture/L2-crate-topology` | — | the end-state crate table + dependency hierarchy + wasm-pure frontier (D15), replacing backend/L2 §1.1 as the one place the workspace topology is named (XV) |

## 8. Migration sequence (Phase 4)

Bottom-up along the dep graph; each step is one landable unit pointing at its
spec section and DEVIATION row. A step closes by: code landed → DEVIATION row
closed → the `[::state]` markers it owns flipped → spec `reviewed` bumped.
Every step leaves the workspace green (`cargo check` + tests); no step depends
on a later step to compile.

| Step | What lands | Decisions | DEVIATION | Notes |
|------|-----------|-----------|-----------|-------|
| M0 | CI wasm check for the already-pure crates (`link-core`, `link-replica`) | D15 (partial) | V6 (partial) | Locks the frontier before the refactor moves things across it; extended in M8. |
| M1 | **Domain split**: create `posthaste-domain-model` (file assignment per §6.1a); relocate `ConfigError`/`ValidationError` (D13); rename the remainder `posthaste-domain-service`; temporary glob re-export of model from service (owner: this RFC, sunset: M8); re-point `link-contract` + `runtime-contract` to `domain-model` | D10, D13 | V4 | The keystone dependency-breaker; everything above sits on it. Biggest mechanical churn (12 dependents), but 10 of them only see the crate rename thanks to the staged shim. |
| M2 | **Extract `posthaste-contract-core`** on `{domain-model, link-core}`: move the wire types out of `runtime-contract` (ids, `MutationRequest`/`Receipt`, view models, errors, `mutation_args`, `mail_query`); re-point type-only consumers (`link-contract`, `link-wasm`, `authority-runtime` type uses) | D5, D6 | V2 | `runtime-contract` shrinks to the `RuntimeCore` trait + re-exports; the wasm path stops dragging `mail-parser`/`tokio`. |
| M3 | **Dissolve `runtime-contract`**: split `RuntimeCore` into `posthaste-runtime-api` (four traits + umbrella) + `posthaste-client-link` (one trait) **executing the §6.4 method assignment verbatim** (41/9 + dispositions); drop `replay_events`; verify-then-trim the sessionless `open_view`/`subscribe_view` pair; `posthaste-runtime` implements both; re-point `api`/`server`/harnesses (oauth_routes narrows to `&dyn RuntimeAccountApi`); delete the crate. `build.rs` splits into `assembly`/`handle`/`shutdown` while its impl block is rewritten (D29). | D7, D23, D29 | V3 | After this the §6.2d wrong-edges list is empty. |
| M3b | **Pre-rename cleanups**: delete the wrapper-migration residue (D20: `MigrationRuntime`, dead `from_api_bridge_*` constructors, zero-consumer provider traits; move the test bridge to testkit/`cfg(test)`; scrub code `@spec` pointers to the retired plan); move the far-node link wire (`link_router`/`start_backend` + its auth) into the authority-server crate with its own error/auth vocabulary (D24) | D20, D24 | V9, V10 | Before M4 so the rename doesn't churn corpses and the moved wire is renamed once, in place. |
| M4 | **Authority-server naming wave**: `posthaste-link-contract → posthaste-authority-server-link`; `posthaste-authority-runtime → posthaste-authority-server`; `DownFrame → AuthorityServerFrame`; `RuntimeId → AuthorityServerLinkId`; `BackendApi/BackendLink/BackendNode/RemoteBackend/LocalBackend/RuntimeBackendOutbox` per the D17 map; binaries per D18 (`posthaste-authority-server`, `posthaste-runtime`, `posthaste-authority-runtime-server`, `posthaste-client`); fix the crate-doc overclaim | D1, D3, D4, D17, D18 | V1 | Pure rename sweep; done after M2 so each crate is renamed once, in its final shape. Generic "backend" prose (backing stores, third-party) is out of scope. The code sweep extends the D17 map to every identifier embedding the component name (`Backend` struct, `BackendBuild`, `build_backend`, `build_authority_runtime`, `BackendTransportConfig`, `select_backend_link`, module files `backend.rs`/`local_backend.rs`, test files `backend_link_*.rs`, in-code `// spec: docs/replication/backend-link/…` path comments) — specs quote these as current-code facts until this step lands. |
| M5 | **Type the operation vocabulary**: `MessageMutation` moves into `contract-core` and becomes `MailOperation` **including the `revCursor` control arm (D22)**; `MutationRequest` carries it typed; dispatch becomes an exhaustive match at exactly the §6.6 crossings (W1/W1b/W2; the in-process string re-parses collapse); the 5 per-command RPCs collapse into `apply(op: MailOperation)` (D21 as renamed by D34) and REST-retry idempotency is verified/added; `MailOperation::fold_effect()` lands in contract-core and both folds (wasm client, near-node outbox) re-point to it (D34); delete `WireMutationId`; the settlement arms are already trimmed in specs (D16 — verify code matches: live set is `Accepted`/`Confirmed`/`Failed`); assert Q4's dep-drop (`link-wasm` → `link-core + link-replica + contract-core`). `backend.rs` splits (reads/commands/pub-sub) while its dispatch is rewritten (D29). | D8, D11, D12, D16, D21, D22, D29 | V2 (closes) | The root fix of the "dirtiness"; last of the topology moves because it touches every wire crossing. |
| M5b | **Far-node seam symmetry (D33)**: produce the per-method assignment of the 49-method `AuthorityServerLink` trait (Api vs Link buckets per D33's rule; dispositions reported before code), then split into `AuthorityServerApi` + `AuthorityServerLink`; `LocalAuthorityServer`/`RemoteAuthorityServer` implement both; runtime's `ReadCache` consumes the Api half | D33 | V13 | After M5 (apply_mail_operation exists) and M4 (names settled). |
| M6 | **`OptimisticReplica` seam + headless-clean layering (D35a, D36)**: extract the trait in link-core; lift the version-gated retire invariant into the engine; split `entity_store.rs` into mechanism vs projection layers (P1 promoted); verify-and-lift any runtime-overlay duplication of the projection layer; `EntityStore` and the runtime outbox compose the shared kernel | D9, D35, D36, D37, D38 | V7 | Semantics unchanged (R2: one owner, two views); headless client = layers 1+2 only; views.rs classified projection-vs-adapter (D37); end-state is ONE projector — unshareable pieces reported with reasons (D38). |
| M7 | **Namespace & hygiene wave**: delete `posthaste-store`'s domain re-exports (D14), the authority-server near-node shim and the server api-facade/`pub mod oauth`/`pub mod supervisor` (D19); move `DaemonSettings` resolution into `posthaste-config` + drop `read_app_toml` (D25); delete `DaemonRuntimeTuning` (D26); extract the shared node-assembly helper + fail-closed `LinkAuth` constructor (D27); unify the query tokenizer into domain-service (D28) | D14, D19, D25, D26, D27, D28 | V5, V8, V11, V12 | Mechanical; each item independently landable. |
| M9 *(next wave)* | **Generic link-end engines (D40/D41/D42)**: extract `LinkFarEnd<Frame, LinkId>` (registry + broadcast/route + cursor replay) and `LinkNearEnd` (Transport-generic + resilience policy) from the two custom far-ends and the Rust near-end; instantiate per seam; land the lifecycle-debt fixes (deadlines, jittered reconnect, reconciler) INSIDE the shared engine, once; move the web client's near-end behind the wasm boundary (TS keeps a zero-policy IO shim, D41); `RuntimeSessionId → RuntimeLinkId` + session→link-connection protocol vocabulary (D42); replica-family crate renames `link-core → replica-core`, `link-replica → replica-projector` + link-wasm naming decision + CI frontier list update (D43). | D40, D41, D42 | *(row opens at drain)* | After M8; M6 is the prerequisite. |
| M8 | **Frontier complete**: extend the M0 CI check to `domain-model` + `contract-core`; remove the M1 glob shim (sunset) | D15 | V6 (closes) | The refactor's definition of done: the wasm client's dep closure is exactly the four frontier crates + `link-wasm`. |

Sequencing invariants: M1 → M2 → M3 strictly ordered (each re-points the
next); M3b/M4 anytime after M3; M5 after M2+M4; M6/M7 independent after M5; M8
last. Rejected orderings: renaming first (M4 before M2) would rename a crate
that is about to lose half its contents — every moved type would churn twice.

## 9. Execution protocol & invariants (Phase 5)

Baseline: this directory is the **jj workspace `architecture-cleanup`**
(renamed from `multi-runtime` 2026-07-02; `.workspaces/` is git-excluded
because jj owns these trees). Everything — code and specs — is jj-tracked;
the drain history already lives in jj changes (phase-0 scaffold at
`tknpowlq`). Git-side reference point in the main history: `113479c2d`.

**Landing ritual is jj-native:** one jj change per execution unit —
`jj commit -m "refactor(M1): … — closes V4"` on completion; review via
`jj diff`; rollback via `jj abandon`/`jj restore`. No nested git repos.

**Driver setup (2026-07-02):** implementation units are executed by headless
`pi` sessions (`pi -p --provider umans --model umans-glm-5.2 --session-id
m<step>-<unit> …`), one session per unit so corrections continue with context.
The orchestrator (Claude) writes each unit brief from the DEVIATION row + spec
sections, reviews the jj diff, runs the gates, and does the RFC/DEVIATION/
marker bookkeeping — the implementing agent never edits the RFC or DEVIATION
register, and never runs `jj` (the orchestrator owns the change boundary).
Parallel pi sessions only for disjoint-scope units.

**Per-unit landing ritual.** Read the DEVIATION row (current + end) → implement
→ run the gates → close the row → flip the `[::state]` markers the unit owns →
bump the affected specs' `reviewed` → commit referencing the M-step and
DEVIATION row (e.g. `refactor(M1): split posthaste-domain — closes V4`).

**Gates (every step leaves the workspace green):**
- `cargo check --workspace` + `cargo test --workspace` (default-members)
- `cargo check --target wasm32-unknown-unknown` for the frontier crates
  (from M0 on; the four-crate set from M8)
- contract artifacts regenerated and committed whenever the wire changes
  (`openapi.json`/`asyncapi.json` never stale at a commit boundary — see the
  wire invariant below)
- web/desktop clients build (`bun`/app tests) on steps touching the wire or
  bins

**Hard invariants during M0–M8:**
1. **The `/v1` wire may evolve — but only atomically.** The app is
   unpublished with no external subscribers (decided 2026-07-02), so no
   compat shims, no dual-format windows, no freeze: M5's `MailOperation`
   takes its *natural* serde encoding rather than mimicking `{name, args}`.
   The discipline that replaces the freeze: any unit that changes the wire
   updates the server, regenerates + commits `openapi.json`/`asyncapi.json`,
   and re-points the TS client and wasm boundary **in the same landing unit**
   — the wire never disagrees between components at a commit boundary. This
   license expires at first publication; from then on XV (strict in, additive
   evolution, deprecation windows) governs.
2. **Bin renames propagate to deployment (M4/D18).** `posthaste-wizard`
   (`render.rs`/`lib.rs`/`fetch.rs` reference bin names), the systemd/launchd
   service units it writes, `install.sh`, and release packaging all carry the
   old names — rename them in the same unit or ship compat symlinks with a
   sunset. A missed one breaks installs silently.
3. **Workspace manifests follow every crate move**: `[workspace] members` +
   the explicit `default-members` list in the root `Cargo.toml`, plus
   `mkdocs.yml` nav for any spec-path change (nav was already re-pointed to
   `authority-server/` + `architecture/` on 2026-07-02).
4. **Specs move with code, not after.** A unit that changes a name/boundary
   updates the affected durable spec section (flip markers, bump `reviewed`)
   in the same commit; the DEVIATION register is the only place current-vs-end
   may disagree.

## 10. Deferred pain points (M9+ candidates)

Noted during execution reviews; each needs its own ratification pass before
becoming an M-step. Seeded 2026-07-02 from evidence already in hand:

| Ref | Pain point | Evidence | Tenet |
|-----|-----------|----------|-------|
| P1 | ~~M9+ candidate~~ **promoted into M6 by D36** — `link-replica/entity_store.rs` 1800-LOC fusion of replica mechanism + view projection; the D36 layer-1/2 split executes it | client audit (git `e5eae6229`); §6.3 | I; XXI |
| P2 | `apps/web` runtime adapter: `entityStoreAdapter.ts` 932 LOC, 3 module-level singletons, `isEntityStoreAdapterActive()` runtime flag splits invalidation ownership across two code paths | client audit; `handlers.ts:96` | XXI; XIV |
| P3 | Lifecycle debt not absorbed by M-steps: push backoff/jitter/breaker, supervisor inline sync await, IMAP timeouts + allocation bounds, web reconnect backoff, snooze wall-clock | `docs/issues/L2-runtime-lifecycle-debt.md` rows 4-6, 8, 10 | V-VIII, XIX |
| P4 | `SortKey.received_at: String` — ISO-8601 lexicographic ordering of server wall-clock at the replica boundary | client audit; `entity_store.rs:96` | XXIII |
| P5 | Timing-flaky test under parallel load: `rapid_mutation_burst_coalesces_provider_sync_triggers` (authority-runtime sync-coalescing window) failed once during M1's full-workspace `cargo test`, passes 3/3 isolated — a real-clock race window in the test | M1 gate run 2026-07-02 | XXIV; II (no clock seam in the test) |
| P6 | Test suites leak `std::env::temp_dir()` directories with no cleanup guards — repeated `cargo test --workspace` runs accumulated 6,853 dirs (2.7 GB) until `EDQUOT` on the 8 GB dev-VM rootfs; test runs are not idempotent in resource terms | M2 gate runs 2026-07-02 | VII (release what you acquire); XXIV |
| P8 | REST direct-apply (`apply(op)`) has no idempotency ledger — retries rely on the ops' inherent set-semantics (union/replace/destroy). Safe for the current 14-variant vocabulary; becomes a bug the day a non-idempotent variant joins. Guard idea: a vocabulary-addition checklist item, or a dedup ledger at M9's far-end engine | M5 idempotency verdict 2026-07-02 | X |
| P7 | Cargo target sprawl vs the 50 GB home quota: four workspace `target/` dirs held ~37 GB and hit `EDQUOT`, killing two agents mid-gate. Dev infra needs a deliberate story — shared `CARGO_TARGET_DIR` per machine, sccache, or per-workspace `cargo clean` lifecycle tied to jj workspace add/forget | M3/D25 launches killed by errno 122, 2026-07-02 | VII (bound what workspaces create) |

**Cross-references that live outside this workspace until merge:** the
superseded spec tree and `docs/issues/*` are in the main tree; the eph docs the
durable specs reference (`DESIGN-L2-deployment-topology`, the undo-redo pair,
`PLAN-L2-testkit-roadmap`, `PLAN-L2-client-link-unification`,
`DESIGN-L2-release-channels`) were copied into this workspace's `docs/eph/` on
2026-07-02 — dedupe at merge. Lifecycle debt (audit findings) is registered at
`docs/issues/L2-runtime-lifecycle-debt.md` with per-M-step opportunistic-fix
markers; it is *not* part of this RFC's scope.
