---
scope: L2
summary: "Forward design: productize the deployment topology over the realized link mechanism — the control-pane UI editing the topology config, the client↔runtime transport selector, the optional Tauri-native IPC node, the split-runtime hardening, and the multi-runtime fan-in preset"
modified: 2026-07-01
reviewed: 2026-07-01
lifecycle: ephemeral
type: DESIGN
depends:
  - path: docs/replication/L1
    section: "10. Deployment topology"
  - path: docs/replication/backend-link/L1
    section: "3.1 Runtime identity and fan-in"
  - path: docs/replication/backend-link/L2
    section: "7. The build seam and role binaries"
  - path: docs/replication/backend-link/L3
    section: "5. Hardening (partial)"
dependents: []
---

# Productizing the deployment topology

The topology mechanism is realized: the build seam (`build_backend` /
`build_runtime`), the lean role binaries (`posthaste-backend` / `posthaste-runtime`
/ `posthaste-daemon`), config-selected transports, and authenticated remote links
are landed and folded into [replication L1 §10](../replication/L1.md) and
[backend-link L2](../replication/backend-link/L2.md). What remains is making it a
**default, productized deployment** rather than a dogfoodable split. When a piece
lands, fold it into the durable section and remove its `[::state]` marker.

## 1. Remaining slices

- **Client↔runtime transport selector.** The client-link transport config
  (remote runtime URL; web vs desktop client shell), reconciled with the WASM
  replica effort. This is the symmetric twin of the backend-link `Remote`
  selector, on the client seam.
- **Control pane.** The desktop UI that edits the topology config — enable/point
  each scope and link, presets as shortcuts, restart-to-apply. Topology switching
  lives only here (`control-is-the-only-switcher`); the lean binaries are
  fixed-role and a change applies by restart (`switch-by-restart`), not hot-swap.
- **Tauri-native IPC (optional, last).** Run the link node natively in the
  control app's Rust process with the webview as a pure view over IPC, retiring
  the embedded localhost HTTP server for the bundled case. An optimization for
  the bundled case only — it never gates the topology, and the app still serves
  HTTP when a remote/web client is configured (`http-universal-transport`).

## 2. Policy surface

Expose the read-cache policy (static: passthrough vs retaining) in the control
config alongside the per-link transport switch. The adaptive RTT-driven policy is
a later refinement, not built now.

## 3. Split-runtime hardening

Dogfood-driven hardening of the split runtime ([backend-link L3 §5](../replication/backend-link/L3.md)):

- eviction under storage pressure (cache bounds / LRU);
- `RuntimeCoverage` reporting what a split runtime holds;
- reconnect / snapshot recovery on the down-channel;
- carrying the account id on assertions to avoid cross-account over-eviction;
- sibling view replicas (detail / conversation) beyond the mail-list;
- role-move optimism (archive / trash / moveToRole), which needs account
  role→mailbox resolution.

## 4. Multi-runtime fan-in

A new preset, now specified but not yet realized: **multiple runtime near nodes
share one remote backend** over authenticated runtime↔backend links. The
spec lives in [replication L1 §10](../replication/L1.md) (the `Hosted backend,
multi-runtime` preset) and [backend-link L1 §3.1](../replication/backend-link/L1.md)
(`RuntimeId`, the per-runtime registry, `settlement-routed-to-origin-runtime`,
`per-runtime-idempotency`, `runtime-credential-per-runtime`). The single-runtime
hosted-backend is the N=1 case.

Why it is a distinct slice, not free from the existing mechanism: the far node
today is built for one runtime — `LocalBackend::subscribe` emits only
`DownFrame::Base` (never `Settlement`), the `Backend` far node tracks no
`RuntimeId`, and `link_router` authenticates with a single shared bearer. Fan-in
needs the far node to attribute mutations and route settlement **per runtime**.
Realizing the preset, in order ([backend-link L3 §6](../replication/backend-link/L3.md)):

1. a per-runtime registry on the `Backend` far node (`RuntimeId` →
   `(ClientMutationId → RuntimeMutationId)` + settlement-routing sink), mirroring
   `posthaste-runtime/src/sessions.rs`'s `mutations_by_client_id`;
2. per-runtime credential authentication in `link_router`, deriving `RuntimeId`
   (the shared bearer stays valid only for the single-runtime remote case);
3. emitting `DownFrame::Settlement` on the down-channel and routing it to the
   originating runtime's stream — `DownFrame::Base` stays a broadcast;
4. `(RuntimeId, ClientMutationId)` dedup at `forward_mutation`.

The co-located path stays single-runtime by construction, so these additions are
gated to the remote transport and leave `colocated-unchanged` intact. Coverage
working-set (`LinkCoverage::WorkingSet`) becomes more valuable here — broadcasting
every assertion to every runtime is correct but wasteful — but remains the
existing hardening item, not a correctness gate for fan-in.
