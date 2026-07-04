---
scope: L2
summary: "Forward design: productize the deployment topology over the realized link mechanism — the control-pane UI editing the topology config, the client↔runtime transport selector, the optional Tauri-native IPC node, and the split-runtime hardening (cache bounds, coverage reporting, reconnect/snapshot recovery, account-scoped eviction)"
modified: 2026-06-24
reviewed: 2026-06-24
lifecycle: ephemeral
type: DESIGN
depends:
  - path: docs/replication/L1
    section: "10. Deployment topology"
  - path: docs/replication/backend-link/L2
    section: "7. The build seam and role binaries"
  - path: docs/replication/backend-link/L3
    section: "5. Hardening (partial)"
dependents: []
---

# Productizing the deployment topology

> **Status: REFERENCE (design note).** Forward design over the realized link
> mechanism (the build seam / role binaries exist). The productization it
> describes — control-pane topology UI, transport selector, Tauri-native IPC
> node, split-runtime hardening — is a forward menu, only partly realized; not
> tracked by a migration ledger. Treat as a design reference, not a shipped plan.

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
