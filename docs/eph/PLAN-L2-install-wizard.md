---
scope: L2
summary: "Plan for the install wizard: a local, one-shot, CLI (later GUI) installer for advanced users to provision the role a chosen topology needs (posthaste_daemon / posthaste_backend / posthaste_runtime_daemon), generate its TLS material, bootstrap the [tls]/[link] config + a systemd/launchd unit, then be deleted. Wiring stays the user's job in the apps' own control pane. First slice (headless CLI + in-daemon TLS) is realized in `crates/posthaste-wizard`; the config-1/config-2 client UX it ultimately serves is still in flight."
modified: 2026-06-30
reviewed: 2026-06-30
lifecycle: ephemeral
type: PLAN
depends:
  - path: docs/eph/PLAN-L2-self-host-topologies
  - path: docs/eph/DESIGN-L2-deployment-topology
  - path: docs/replication/L1
    section: "10. Deployment topology"
dependents: []
---

# Install wizard: one-shot local composition installer

## 0. Status

**First slice realized (2026-06-30).** `crates/posthaste-wizard` (`provision`)
now writes a node's `app.toml` (`[daemon]`/`[tls]`/`[link]`), generates a local
CA + server leaf (a CA+leaf, not a flat self-signed cert — rustls/webpki rejects
a cert that is both trust anchor and end-entity), emits a systemd unit, and
prints the client `remote` connection profile. The unblocking dependency turned
out to be **in-daemon TLS**, now built (opt-in `[tls]` cert/key in
`posthaste-api`, served via `serve()`; client reaches it over HTTPS with a
macaroon token). Live-verified end-to-end: wizard-provisioned TLS node →
`posthaste_daemon serve` → authed `GET /v1/accounts` over HTTPS = 200; no token = 401;
plaintext + untrusted-CA rejected.

**Still ahead** (the original deferral, narrowed): release-packaging the
backend/runtimed/thin-desktop binaries for distribution, the in-app control pane
for ongoing wiring, the config-1 thin-web client, a GUI face, and ACME/rotation.
The wizard installs locally-built binaries today (the `--exec` path); fetching
distributed artifacts is the packaging follow-on. See §5 for the dependency order.

### Config surface the wizard writes

- `[daemon] bind` / `require_auth` (default true) / `allowed_hosts` — the last
  admits remote `Host` headers past the DNS-rebinding guard (a wildcard bind
  admits none on its own).
- `[tls] cert` / `key` — PEM paths; present ⇒ the daemon serves HTTPS. Absent ⇒
  plaintext loopback, unchanged. Both keys are required together (fail closed).
- `[link] serve` / `token` / `backend_url` — the backend role serves the link
  with the shared `token`; the runtime role presents it to `backend_url`.

## 1. Context — what already exists

The topology *mechanism* is realized (see
[self-host-topologies PLAN](PLAN-L2-self-host-topologies.md) +
[deployment-topology DESIGN](DESIGN-L2-deployment-topology.md) +
[replication/L1 §10](../replication/L1.md)):

- **Build seam** (`build_backend` / `build_runtime` / `build_remote_runtime` in
  `posthaste-authority-runtime/src/build.rs`).
- **Three role binaries**, config-selected:
  - `posthaste_daemon` — the daemon; full in-process backend+runtime+`/v1`+renderer by
    default, OR a lean near-node over a remote backend when `[link] backend_url`
    is set. **The only role binary packaged in the release today.**
  - `posthaste_backend` (`src/bin/backend.rs`) — the lean far node; serves only
    the authenticated link. Built, not yet packaged for distribution.
  - `posthaste_runtime_daemon` — the lean remote-runtime daemon (near node over a
    remote backend, serves `/v1`, links no store/engine/imap). Built, not yet
    packaged.
- **Link auth** — bearer-token + macaroon HMAC on both the `/v1` perimeter and
  the runtime↔backend link, with an attenuating token CLI.
- **Split is exercised over real HTTP** — `tests/backend_link_split.rs`.

What does **not** exist: a way for an advanced user to install *only the
components their topology needs* on a given machine (without the whole embedded
bundle) and then point them at each other. Today advanced self-hosting means
hand-fetching binaries, hand-writing `[link]` config, and hand-writing a
systemd/launchd unit. The wizard removes that friction.

## 2. The wizard (concept, as agreed)

A **local, one-shot installer** — a single app with **GUI and CLI** faces (same
binary). Not a manager, not stateful, not for updates: install + delete.

- **Per-machine, local-only.** Backend on a VM? Run the wizard *on the VM*. No
  cross-machine pairing, no remote discovery magic.
- **Advanced-only.** Newbies download the main embedded app; the wizard is for
  users who want a split / headless / remote topology.
- **Installs components à la carte**: the role binary for this machine's role
  (`posthaste_daemon`, `posthaste_backend`, or `posthaste_runtime_daemon`), optionally
  `posthastectl`, and optionally the **thin-client desktop** (the
  `embedded-server`-off build — a GUI client with no runtime baked in). It
  fetches ONLY what the chosen role needs — a machine ships only the code it
  runs (the self-host PLAN's organizing principle).
- **Bootstraps config + service**: writes the starter `app.toml`
  `[link]`/bind/auth section for the role and generates the systemd/launchd unit.
  Then the wizard can be deleted; the components are self-sufficient afterward.
- **Wiring is the user's job, in the apps' own settings** — the existing
  "control pane" (the desktop UI that edits topology config;
  [deployment-topology DESIGN §1](DESIGN-L2-deployment-topology.md)). The wizard
  does NOT manage connections over time; it does not need to be kept around for
  the app to keep working.

## 3. Map to the existing topologies

| Topology (self-host PLAN) | Wizard installs per machine | Config it writes |
|---|---|---|
| **Main / bundled** (default) | *(no wizard — main app)* | — |
| **Headless CLI-only** | `posthaste_daemon` + `posthastectl` | daemon bind + auth |
| **Config 2** (remote backend + local runtime + many clients) | VM: `posthaste_backend` (or `posthaste_daemon` full daemon); client box: `posthaste_daemon` / `posthaste_runtime_daemon` (near node) + opt. `posthastectl` + thin client | VM: backend bind + link auth; client: `[link] backend_url` + token |
| **Config 1** (backend+runtime remote, thin web clients) | server: `posthaste_daemon`; client: web app (WASM replica) + opt. `posthastectl` | server: bind + link auth + `serve_web`; client: connection profile |

## 4. Gap analysis (grounded, reconciled with what's built)

| Step | Built? | Notes |
|---|---|---|
| Build seam + role binaries | ✅ | But only `posthaste_daemon` is **packaged in the release**; `posthaste_backend` + `posthaste_runtime_daemon` are built, not distributed. |
| Link auth (macaroon + bearer, attenuating token CLI) | ✅ | |
| Config-selected daemon / near-node (`[link] backend_url`) | ✅ | |
| Split over real HTTP | ✅ | `tests/backend_link_split.rs`. Live two-host shakeout remains (self-host PLAN Phase 1 step 5). |
| **Wizard** (fetcher + config bootstrap + service unit) | ❌ | New tool. The easy part — it's a fetcher/installer. |
| **Service management** (systemd/launchd unit generation) | ❌ | No owner today. The wizard (one-shot: write the unit, then delete) is the natural home. |
| **Control pane** (in-app topology / connection editor) | ❌ | [deployment-topology DESIGN §1](DESIGN-L2-deployment-topology.md) remaining slice. The wizard's "configure in GUI settings" depends on this existing. |
| **Thin client** (web WASM replica; optional thin-desktop) | ⚠️ in flight | self-host PLAN Phase 2. `connections.json` store + `hostHeader` validation are seeded in `client_connection.rs`; Phase B runtime incomplete ("degrades"). |
| **CLI config surface** (`posthastectl config` / `account` / `connect`) | ❌ | `posthastectl` today is `events` / `watch` only. Headless config = hand-edit `app.toml` until a CLI config surface exists. |
| **Component updates** for headless / role binaries | ❌ | Wizard disclaims updates. Main app auto-updates (Tauri); role binaries don't. Needs an owner. |

## 5. Why deferred — the dependency order

The wizard is a **packaging + bootstrap layer** over the topology mechanism. It
is blocked on the very slices it serves: it can install a thin-client desktop
that doesn't yet work, and its "configure in GUI settings" points at a control
pane that doesn't yet exist. Per
[self-host-topologies PLAN](PLAN-L2-self-host-topologies.md), those land in:

- **Phase 1** (config-2 productization): connection-config UX, daemon packaging,
  TLS, live two-host shakeout.
- **Phase 2** (config-1): WASM replica finished + defaulted, client-link auth,
  thin web client.
- **Phase 3** (lean polish): control-pane picker, surfaced-failure UX,
  reconnection/offline, self-host docs, *optionally* a thin-desktop build.

The wizard belongs alongside Phase 3: it *packages* the already-built role
binaries (`posthaste_backend`, `posthaste_runtime_daemon`) and *bootstraps* the config
the control pane then edits. Building it before its dependencies means installing
components the apps can't yet wire — a frustrating dead end.

## 6. Open scope questions (unresolved — decide before building)

1. **Headless config story** — hand-edit `app.toml` (acceptable for advanced),
   or does `posthastectl` need real `config` / `account` / `connect` commands?
2. **Service-management owner** — the wizard writes the systemd/launchd unit as
   part of install (one-shot, then delete), or `posthastectl service install`
   (repeatable)? Lean: wizard-writes-unit, keeping `posthastectl` purely an ops
   tool.
3. **Browser client / PosthasteWeb** — in the wizard's component menu, or is
   thin-desktop the only GUI for now? (self-host PLAN rec: web-app-first.)
4. **Uninstall** — does the wizard remove components it installed, or
   install-only (advanced users clean up themselves)?
5. **Updates** — confirm out of scope for the wizard; then which component owns
   updating role binaries on a VM?

## 7. First slice (when resumed)

Smallest vertext that proves the spine without the blocked dependencies:
**headless CLI-only** topology — wizard binary (GUI+CLI) + component fetcher +
`[link]`/bind/auth config bootstrap + systemd/launchd unit generation, for
`posthaste_daemon` + `posthastectl` on one machine. No control-pane dependency (the
daemon is configured via the wizard-written `app.toml`), no thin-client
dependency. Expand to config-2 / config-1 as the control pane + thin client land.
