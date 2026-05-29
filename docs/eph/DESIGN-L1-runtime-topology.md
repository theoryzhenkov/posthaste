---
scope: L1
type: DESIGN
lifecycle: ephemeral
summary: "Runtime topology: one server, two lifecycles (embedded vs daemon) behind one HTTP contract"
modified: 2026-05-29
reviewed: 2026-05-29
depends:
  - path: docs/eph/PLAN-L1-public-api-platform
  - path: docs/L2-transport
dependents:
  - path: docs/eph/PLAN-L1-public-api-platform
---

# DESIGN: Runtime topology

## Why

Posthaste must serve two audiences from one product: the casual user who installs
a `.dmg` and runs a normal desktop app, and the integrator who wants custom
clients or agents driving the backend directly. This doc records *how* the process
topology supports both, because that decision shapes the trust model (PLAN P4) and
the deployment story. It is the design record for the embedded-vs-daemon question.

## Current state (2026-05-29)

The flexible architecture mostly already exists — it was just never named:

- The backend is a reusable async server: `posthaste_server::start_server` returns a
  `ServerHandle` (`crates/posthaste-server/src/lib.rs:91`), with a
  `bind_address_override` and an optional `frontend_dist`.
- **Embedded today**: the desktop app calls `start_server` *in-process* with
  `bind_address_override: "127.0.0.1:0"` — an **ephemeral port** the OS assigns
  (`apps/desktop/src/lib.rs:374`) — then injects the chosen port into the webview as
  `window.__POSTHASTE_PORT__` (`lib.rs:338`) so the frontend discovers the backend.
- **Daemon today**: the same code is a standalone binary, `[[bin]] name = "posthaste"`
  (`crates/posthaste-server/Cargo.toml:10`), binding the configured `[daemon]` address
  (default `127.0.0.1:3001`, `config.rs:81`), and able to serve the frontend statically
  (`frontend_dist`) for browser mode.

So the desktop app is **already just an HTTP client of its own backend**. "In-client
vs daemon" is not two architectures — it is two *lifecycles* of one server speaking
one contract. This is exactly why the OpenAPI plan is coherent: the contract is the
product, and every mode speaks it.

## Decision

**One server implementation; two lifecycles; auto-detected, not a forced switch.**

1. **Embedded is the default.** Casual users never choose a mode. The app spawns the
   in-process server on an ephemeral loopback port (today's behavior), which dies when
   the app quits. Zero configuration.
2. **Daemon is opt-in.** A setting ("Run Posthaste in the background for continuous
   sync") starts the standalone `posthaste` binary as a background/login item. Its
   value proposition is explicit: **sync while the app is closed + a stable endpoint
   for programmatic access.** Users who want neither are well served by embedded mode.
3. **The app auto-detects.** On launch: try to connect to a running daemon → if present,
   connect to it; if absent, spawn the embedded server. The "switch" is an override, not
   a decision imposed on everyone.
4. **Integration targets the daemon.** Embedded mode's ephemeral port is a moving target
   (good for secrecy, useless for external clients). External clients/agents connect to
   the daemon's stable endpoint. This is the honest contract: programmatic access implies
   the background daemon.

### Mode matrix

| Mode | Lifecycle | Transport | Consumers | Auth posture |
|---|---|---|---|---|
| **Embedded** (default) | dies with app | ephemeral loopback port, injected into webview | bundled UI only | port-secrecy; minimal |
| **Daemon** (opt-in) | background / login item | fixed port + documented port-file | bundled UI *and* external clients/agents | token + `Origin`/`Host` check (PLAN P4) |

The two lifecycles line up cleanly with the two security postures: secrecy is adequate
for an unguessable ephemeral port serving only the first-party webview; a fixed,
reachable daemon port needs real auth.

## Critical invariant: single writer on the local store

The SQLite replica + state dir must have **exactly one** backend process writing it. If
a daemon is running **and** the app also spawns its embedded server, two processes write
the same DB → lock contention / corruption. The auto-detect handshake is therefore a
**safety lock**, not a convenience:

- The daemon writes a port-file in the state dir, e.g. `daemon.json` = `{ port, token }`.
- The embedded server refuses to start if that lock is held; the app connects to the
  daemon instead.
- The port-file doubles as the **documented discovery mechanism** for external clients,
  so integrators read `{port, token}` rather than hard-coding `3001`.

Getting this handshake right is the one thing that must not break.

## Relationship to the API plan

- This topology is *why* the OpenAPI contract is the product surface (PLAN "Why").
- Auth (PLAN P4) attaches to **daemon mode**: the fixed port gets the loopback token +
  `Origin`/`Host` check that closes the browser/CSRF/DNS-rebinding vector; the
  capability-scoped authz for agents lands here too. Embedded mode, with its injected
  random port, needs little of this.
- Supports deferring the heavy trust model: embedded stays localhost-only and unexposed,
  so P4 can be a fast-follow — but the cheap origin-token guard for daemon mode should
  ship whenever daemon mode is first reachable.

## Deferred / open

- **macOS background mechanism**: launchd agent with on-demand/idle-exit (so "background"
  ≠ "always burning RAM") vs. a simple login item. Decide at daemon-mode implementation.
- **Windows/Linux service wrappers**: out of scope until daemon mode ships.
- **Port-file schema + token minting**: specify alongside PLAN P4.

## Related

- `docs/eph/PLAN-L1-public-api-platform` — the contract this topology serves; P4 trust model.
