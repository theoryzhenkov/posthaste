---
scope: L2
summary: "Phase-0 audit + phased plan for productizing the two self-host topologies (remote-backend/local-runtime/many-clients and remote-runtime/thin-web-clients) over the already-landed build seam — with the leanness finding that role binaries are runtime-lean but not compile-lean, so the build matrix needs a crate/feature boundary between store+engine roles and the near-node/client roles"
modified: 2026-06-25
reviewed: 2026-06-25
lifecycle: ephemeral
type: PLAN
depends:
  - path: docs/replication/L1
    section: "10. Deployment topology"
  - path: docs/replication/backend-link/L2
    section: "7. The build seam and role binaries"
  - path: docs/eph/DESIGN-L2-deployment-topology
dependents: []
---

# Self-host topologies: Phase 0 audit + phased plan

The goal is to productize PostHaste for **lean self-hosting** in two concrete
topologies, over the link mechanism that has already landed. This doc records
the Phase 0 ground-truth audit (run 2026-06-25 against the working tree) and the
phased plan it implies. It supersedes the README/architecture-level reasoning the
plan was first sketched from: most of what was assumed "coded but not exercised"
is in fact **built and exercised by integration tests** — the remaining work is
real packaging, the compile-time leanness boundary, and config-1's client layer.

The two target topologies:

- **Config 2 (the near target):** remote **backend** daemon owns the store +
  provider and always syncs; a **local runtime** near-node connects over the
  authenticated backend link; **many local clients** share its drafts / smart
  mailboxes / view state via the session model. `R↔B` is remote, `C↔R` is local.
- **Config 1 (further out):** backend **and** runtime on the server; **thin web
  clients** connect over an authenticated client link. Needs the WASM replica
  finished + defaulted and client-link auth.

## 1. Phase 0 findings (ground truth)

### What is already built — and exercised

- **The build seam exists** in `posthaste-authority-runtime/src/build.rs`:
  `build_backend` (far node alone), `build_runtime` (runtime over a backend
  link), `build_backend_node`, and `build_remote_runtime` (a backend-less near
  node over `BackendTransportConfig::Remote`).
- **Config-2 is wired end-to-end in the default `posthaste` binary.**
  `start_server` (`startup.rs`) reads `[link] backend_url`: when present it
  calls `build_remote_runtime` and serves `/v1` to local clients as a **lean
  near node** (no local backend); when absent it builds the full in-process
  daemon. So the same binary is *both* the daemon and the runtime near-node by
  config — there is **no separate `posthaste-runtime` / `posthaste-daemon`
  binary** despite [deployment-topology DESIGN](DESIGN-L2-deployment-topology.md)
  and [backend-link L2 §7](../replication/backend-link/L2.md) naming three. Only
  two binaries exist: `posthaste` (daemon/near-node, config-selected) and
  `posthaste-backend` (`src/bin/backend.rs`, the lean far node that serves only
  the link — no `/v1`, no renderer).
- **The split is exercised over real HTTP**, not mocks:
  `tests/backend_link_split.rs` stands up a real backend over a `TcpListener` +
  `axum::serve` + `link_router`, points a `build_remote_runtime` near-node at it
  via `RemoteBackend` over `http://…`, and drives writes/reads/mutations across
  the link into the authoritative store. `lean_remote_runtime_drives_the_backend_
  over_the_link`, `remote_runtime_serves_a_mail_list_view_from_the_backend`, and
  `remote_runtime_forwards_a_mutation_into_the_backend_store` all cover this.
- **Link auth exists:** bearer-token + macaroon HMAC root key on both the `/v1`
  perimeter (`auth.rs`) and the runtime↔backend link (`link.rs`), with a token
  CLI (`token_cli.rs`, attenuation) — aligning with `feature/capability-tokens`.

**Net:** config-2's *mechanism* is built and integration-proven. The transcript's
"coded but not exercised end-to-end" is stale at the test level; it is still true
at the **live two-host / ops level**.

### The leanness gap (the real Phase 0 finding)

`posthaste-authority-runtime` **unconditionally** depends on `posthaste-store`,
`posthaste-engine`, and `posthaste-imap`; `posthaste-server` does too. There are
**no `[features]` gates** in either crate. So the "lean near node" is lean only
at **runtime** (it opens no store, runs no provider sync) — it still **compiles
and links** the entire store+engine+imap graph. The `posthaste-backend` far node
and the near-node `posthaste` share one fat dependency graph.

This is the gap between "behaves thin" and "ships thin." The plan's organizing
principle — *a machine ships only the roles it runs* — requires a **crate or
feature boundary** between the store+engine-bearing roles (backend) and the
roles that don't need them (near-node runtime, client). That boundary does not
exist yet and is the substance of Phase 0's deliverable.

### `feature/deployment-modes`

That branch is a **divergent line** at `feat(web): connection-profile store +
dynamic per-profile resolution (Phase B)` — web-app connection-profile work, not
the role-binary work (which is on main). Its server `Cargo.toml` has only the
`posthaste` bin. Treat it as the **client-side connection-profile** input for
config-1/config-2 client UX, not as the source of the build seam.

## 2. The build matrix

| Binary | Role | Links | Should exclude (dormant) | Status |
|---|---|---|---|---|
| `posthaste-backend` | lean far node | store + engine + imap + link-router | renderer/web, runtime view-layer | **built**, not yet compile-lean |
| `posthaste` (daemon) | backend + runtime in-process + `/v1` + renderer host | everything | — (meant to be fat) | **built** |
| `posthaste` (near-node) | runtime over remote backend + `/v1` | currently store+engine+imap too | **store + engine + imap** | **built (runtime-lean only)** |
| thin client = web app | renderer + WASM replica + client-link | authority-runtime, store, engine | — (already JS/WASM) | replica effort in flight |

The matrix's one structural problem is the third row: the near-node should not
link store/engine/imap. Options (decide in Phase 0 close-out):

1. **Feature-gate `posthaste-authority-runtime`** so `build_remote_runtime` and
   the near-node path compile without store/engine/imap (a `backend` feature
   that the daemon/backend turn on, off by default), plus a matching server
   feature. Lowest churn; risk is conditional-compilation sprawl.
2. **Extract a `posthaste-near-runtime` crate** holding only the near-node build
   + read-cache + outbox over `BackendApi`, depending on link-contract/core but
   not store/engine. Cleanest dependency hygiene; more up-front refactor. The
   near-node already only touches `BackendApi`, so the seam is natural.

Recommendation: **option 2** if the near-node build genuinely only needs
`BackendApi` + link crates (the audit suggests it does); fall back to option 1 if
shared types drag store/engine in transitively.

## 3. Phased plan

**Phase 0 — Audit + build matrix (this doc).** Done: ground-truth audit above +
the matrix. Close-out decision still owed: matrix option 1 vs 2, and whether to
reconcile the three-binary naming in the durable docs down to the two that exist
(or add the named lean binaries as thin wrappers if naming clarity is worth it).

**Phase 1 — Make config 2 a real product** (nearer; mechanism + tests landed).
1. **Compile-time leanness** for the near node — the one piece of new
   architecture; everything else is productization. **[done at the crate level,
   2026-06-25]** Done as a **crate split**, not a feature gate (see decision 1):
   the renderer-facing near node moved to a new **`posthaste-runtime`** crate
   (deps: domain/runtime-contract/link-{contract,core,replica}; **no**
   store/engine/imap). `posthaste-authority-runtime` is now the far node, depends
   on `posthaste-runtime`, and re-exports its surface so hosts keep one import.
   The far crate composes an in-process runtime via the near crate's
   `assemble_runtime`; `mutation_args` is a shared module; the OAuth holdout left
   the near handle (option B — a backend op the server calls directly). No cfg,
   no `dead_code` allow. Workspace + tests green. **Lean binary done
   (2026-06-25):** `posthaste-server` was further split — the `/v1` HTTP platform
   (REST + the client-link runtime sessions, auth/authz/sanitize/openapi,
   `AppState`, serve glue) extracted into a near-only **`posthaste-api`** crate
   (deps: `posthaste-runtime` only); the OAuth-flow routes stay in
   `posthaste-server` on an `OAuthState` merged into the `/v1` router (bundled
   OpenAPI = near + OAuth, lean node serves the OAuth-free doc). New lean daemon
   binary **`posthaste-runtimed`** (= `posthaste-api` + `posthaste-runtime`) runs
   `build_remote_runtime` + serves `/v1`; `cargo tree` confirms it links no
   store/engine/imap. Every source file < 300 lines. **Remaining for config-2:**
   steps 2–5 below (connection UX, packaging, TLS, live two-host shakeout).
2. **Connection config UX**: point a local runtime at a remote backend (URL +
   token) — reconcile with `feature/deployment-modes` connection profiles.
3. **Package the daemons**: `posthaste-backend` + the near-node `posthaste` as a
   binary/container + self-host setup docs ("run as a daemon").
4. **Transport security** for the remote `R↔B` link over LAN/tailnet: TLS +
   token issuance/rotation (bearer+macaroon exist; network-exposure hardening
   and a TLS story do not).
5. **Live two-host shakeout** — the real remaining gate above the integration
   tests: backend on one host, runtime+clients on another, over the network.
6. Pull in the **split-runtime hardening** items already enumerated in
   [deployment-topology DESIGN §3](DESIGN-L2-deployment-topology.md) as they
   surface (eviction/LRU, `RuntimeCoverage`, reconnect/snapshot recovery,
   account-id on assertions, sibling view replicas, role-move optimism).

**Phase 2 — Make config 1 usable** (needs the client optimism layer).
Finish the WASM replica (real-browser validation + robustness + default it for
the remote client), client-link auth (capability/session tokens — align with
`feature/capability-tokens`), web client → remote runtime, validation. The
remote **read** replication noted in `backend_link_split.rs` (down-channel
carrying served rows; L4 §4.3 / W4 coverage) is shared groundwork here.

**Phase 3 — Lean polish.** Control-pane connection-profile picker
([deployment-topology DESIGN §1](DESIGN-L2-deployment-topology.md)),
surfaced-failure UX, reconnection/offline, self-host docs, and *optionally* a
thin desktop-client build (web app covers most of the thin-client need).

## 4. Decisions to settle (Phase 0 close-out)

1. **Build matrix mechanism:** **decided: crate split (extract
   `posthaste-runtime`), landed.** The feature-gate spike worked but produced
   cfg-per-function + a `dead_code` allow — a code smell, and a runtime boundary
   forced through the compiler. A crate boundary is the right unit (a crate is
   wholly shared or not). The near node's only real coupling to the far node was
   the OAuth holdout, resolved by moving it off the handle (option B). Result:
   each crate links exactly what it uses, no conditional compilation. The shared
   replica/outbox spine (`link-replica`/`link-wasm`) was already factored
   correctly; this split brings the runtime crate to the same standard.
2. **Binary naming:** keep the two real binaries and correct the docs, or add
   `posthaste-runtime` / `posthaste-daemon` as thin role-named wrappers for
   operator clarity. *Rec: correct the docs; the config-selected `posthaste` is
   simpler than three binaries.*
3. **Thin client = web-app-first?** *Rec: yes — naturally lean, unblocks both
   configs without the thin-desktop-build problem.*
4. **Link transport security:** TLS termination strategy for the remote
   backend-link (native TLS vs reverse-proxy/tailnet-assumed) for Phase 1.

## 5. Recommended next step

Phase 1 step 1 (compile-time leanness) is the only item that is new architecture;
it is also the cleanest first slice and de-risks the whole matrix. Concretely:
prove whether `build_remote_runtime` + the near-node read-cache/outbox path can
compile without `store`/`engine`/`imap` (option 2 spike), and if so extract
`posthaste-near-runtime`. If that boundary holds, config-2 productization (steps
2–5) is straight packaging + ops work over an already-proven mechanism.
