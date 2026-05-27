---
scope: L1
summary: "Lab suite registry, profiles, drivers, artifacts, command names, and Tauri automation contracts"
modified: 2026-05-27
reviewed: 2026-05-27
depends:
  - path: docs/L0-lab
  - path: docs/L0-testing
  - path: docs/L1-api
  - path: docs/L1-ui
  - path: docs/L1-logging
dependents: []
---

# Lab domain -- L1

## Control-plane components

PostHaste Lab is composed of small explicit components rather than one custom test framework:

| Component | Canonical IDs | Responsibility |
|---|---|---|
| Suite registry | `suite.*` | Maps verification intent to existing test runners, fixtures, platforms, tags, and artifacts |
| Profiles | `profile.*` | Declare isolated config/data/secret roots, env vars, network policy, and cleanup behavior |
| Fixtures | `fixture.*` | Seed deterministic accounts, mailboxes, messages, provider state, and failure scenarios |
| Runners | `runner.*` | Invoke native tools such as cargo, Bun, Playwright, Tauri, and future `posthastectl` commands |
| Drivers | `runner:web.*`, `runner:desktop.*` | Drive web or desktop clients through Playwright, Tauri IPC mocks, or a Tauri bridge |
| Artifact bundle | `artifact.*`, `log.*`, `state.*` | Persist machine-readable run evidence for agents and humans |

The lab CLI should be a thin orchestrator over these components. It should not reimplement cargo, Bun, Playwright, process supervision, or test-selection algorithms when mature tools already provide them.

## Suite registry

A suite registry entry describes *why* and *how* a behavior is verified:

```toml
[suite.api.settings.dev]
level = "integration"
targets = ["daemon"]
runners = ["runner.cargo-nextest.dev"]
tags = ["api", "settings", "fast"]
paths = ["crates/posthaste-server/src/api/settings.rs"]
command = "cargo nextest run -P quick -E 'test(settings_patch)'"
artifacts = ["artifact.junit.settings.dev", "log.backend.jsonl.dev"]

[suite.ui.settings.web.test]
level = "e2e"
targets = ["web"]
profile = "profile.lab.empty.test"
fixture = "fixture.mail.basic.test"
runners = ["runner.playwright.web.test"]
tags = ["ui", "settings", "browser"]
command = "posthaste-lab run suite.ui.settings.web.test"
artifacts = ["artifact.trace.settings.test", "artifact.screenshot.settings.test"]

[suite.desktop.settings.linux.test]
level = "e2e"
targets = ["desktop", "linux"]
profile = "profile.lab.empty.test"
fixture = "fixture.mail.basic.test"
runners = ["runner.tauri-playwright.linux.test"]
tags = ["ui", "settings", "tauri", "linux"]
command = "posthaste-lab run suite.desktop.settings.linux.test"
```

The registry supports selection by explicit suite ID, tag, target, platform, risk profile, and changed files. Changed-file selection must escalate across behavioral boundaries when public API schemas, event payloads, config schema, shared cache keys, or suite fixtures change.

## Command surface

Canonical developer commands are module-oriented and explicit:

```sh
just web dev
just desktop dev
just dev web
just dev desktop
just dev services
just dev smoke
just dev log path
just dev log tail
just dev log query --event http.request.completed
```

Lab orchestration should eventually expose a dedicated CLI:

```sh
posthaste-lab suite list
posthaste-lab verify suite.api.health.dev
posthaste-lab verify --tag lab-smoke
posthaste-lab verify --tag settings --target web
posthaste-lab verify --changed
posthaste-lab launch web --profile profile.lab.empty.dev
posthaste-lab launch desktop --profile profile.lab.empty.dev --runner runner.tauri-playwright.linux.test
```

`posthastectl` is a dev/lab API client, not yet a product CLI:

```sh
posthastectl health wait
posthastectl settings get
posthastectl settings patch --json @settings.json
posthastectl accounts list
posthastectl events wait --resource appSettings.updated
posthastectl fixture load fixture.mail.basic.test
```

The future headless daemon and terminal TUI may promote a stable subset of `posthastectl`, but lab-only fixture mutation and rich diagnostics remain separate.

## Profiles and fixtures

Every lab run uses disposable roots under a run directory:

```text
target/lab/runs/<run-id>/
  manifest.json
  summary.json
  state.config/
  state.data/
  state.secrets/
  log.backend.jsonl
  log.frontend.jsonl
  stdout.log
  stderr.log
  opened-urls.jsonl
  artifact.screenshot.*.png
  artifact.trace.*.zip
  artifact.video.*.mp4
```

Profile IDs describe execution environment and policy:

| ID | Purpose |
|---|---|
| `profile.lab.empty.test` | Empty local profile, no accounts, no real secrets |
| `profile.lab.seeded.test` | Seeded local mail data, deterministic timestamps and IDs |
| `profile.lab.offline.test` | Network disabled except loopback |
| `profile.lab.stalwart.dev` | Local Stalwart provider fixture |
| `profile.lab.upgrade.dev.from:v0.1.0-dogfood.17` | Upgrade/regression profile from an older app state |

Fixtures are explicit products, not hidden setup. They declare seeded accounts, provider behavior, side-effect adapters, and cleanup policy. Real-provider parity remains a separate higher-cost suite; deterministic fixtures must not become the only proof of sync correctness.

## Readiness and error contracts

UI and backend waits use semantic readiness, not sleeps.

Frontend surfaces expose stable markers:

```text
state.app.loading.test
state.app.ready.test
state.app.error.test
state.settings.loading.test
state.settings.ready.test
state.settings.error.test
state.message-detail.ready.test
state.compose.ready.test
```

The DOM representation may use `data-testid` or `data-posthaste-state`, but the suite registry and lab reports refer to canonical state IDs. Loading states that can block a user must have a reachable error state with diagnostic context. Infinite spinners are test failures.

The daemon exposes a minimal product health endpoint; richer lab-only diagnostics remain planned lab contracts:

| Endpoint | Mode | Purpose |
|---|---|---|
| `GET /v1/health` | product and lab | Process/API readiness without sensitive state |
| `GET /v1/lab/health` | lab only, planned | Config root, fixture, account convergence, event stream, and side-effect recorder state |
| `GET /v1/lab/opened-urls` | lab only, planned | External URL requests captured by the lab opener adapter |

When implemented, lab endpoints must refuse non-loopback use and must not expose credentials, message bodies, tokens, or raw provider payloads.

## App drivers

The driver ladder is:

1. `runner.playwright.web.test`: Playwright against built web assets.
2. `runner.tauri-mock.web.test`: frontend tests with Tauri `mockIPC` for IPC behavior.
3. `runner.tauri-playwright.linux.test`: real Linux Tauri app with a feature-gated bridge.
4. `runner.package.linux.test`: packaged Linux artifact smoke.
5. Manual macOS release artifact smoke, until a macOS runner is deliberately introduced.

### Tauri Playwright spike contract

The `tauri-playwright` bridge is acceptable only behind an e2e feature:

- `feature.e2e-testing` enables the optional `tauri-plugin-playwright` dependency and the PostHaste Linux e2e bridge.
- `POSTHASTE_E2E_SOCKET` supplies a private per-run Unix socket path; the test fixture uses the same path as `mcpSocket`.
- The default `/tmp/tauri-playwright.sock` is never used.
- The `playwright:default` capability is included only when the e2e feature selects the e2e capability file.
- `withGlobalTauri` is enabled only in the e2e config override because the app-side bridge uses Tauri events and invoke; normal desktop config keeps the tighter production setting.
- Linux CI runs the Tauri bridge under a real or virtual display (`xvfb-run` or equivalent) with WebKitGTK dependencies installed.
- Normal release builds and DevTools dogfood builds do not include the permission, global Tauri injection, private socket bridge, or bridge marker.
- Initial Linux suites target the first ready Lab surface. Separate settings/message/attachment control is added only after multi-window label handling is proven reliable.

Go/no-go for the spike:

| Outcome | Decision |
|---|---|
| Can launch Linux Tauri with isolated profile and wait for `state.app.ready.test` or a forced first-run `state.settings.ready.test` | Continue |
| Can open settings and wait for `state.settings.ready.test` with screenshot/trace on failure | Continue |
| Can record external URL opener requests without opening a browser | Continue |
| Requires broad production config weakening or global unauthenticated sockets | Stop |
| Multi-window support is unreliable | Keep bridge for main-window smoke only and use web tests for surface routing |

## Artifact manifest

Every lab run writes `manifest.json` and `summary.json`.

`manifest.json` records:

- command and canonical command ID (`cmd.*`)
- suite IDs selected, selection rationale, and per-suite execution records
- commit ID, platform, machine ID, tool versions
- profile and fixture IDs
- environment variables after redaction
- process tree and ports/sockets
- artifact paths

`summary.json` records:

- status: `passed`, `failed`, `skipped`, or `blocked`
- per-suite status, duration, timeout flag, exit code, and stdout/stderr artifact paths
- first failure suite and step
- reproduction command
- important log excerpts
- links to screenshots, traces, videos, and backend/frontend logs

Agents should inspect the artifact bundle before rerunning a failing suite.

## Release relationship

Release promotion should test the artifact that will be published whenever practical. For now:

- Linux packaged smoke can be automated.
- macOS artifacts are ad-hoc signed and manually smoke-tested from GitHub release assets.
- Release checks must prove lab-only bridge/debug controls are absent from release artifacts.

## Assertions

| ID | Sev. | Assertion |
|---|---|---|
| registry-thin-orchestrator | MUST | The suite registry delegates to existing runners and records selection rationale instead of implementing a bespoke test runner |
| disposable-run-roots | MUST | Lab runs use isolated config, data, secret, log, and artifact roots by default |
| semantic-readiness | MUST | UI and backend waits use semantic ready/error states rather than sleeps or network-idle guesses |
| no-infinite-spinner | MUST | User-visible loading states involved in lab suites expose a ready or error state that tests can assert |
| bridge-feature-gated | MUST | Real Tauri automation bridges are compile-time feature gated and absent from normal release artifacts |
| private-e2e-socket | MUST | Tauri automation uses a private per-run socket or equivalent non-predictable local channel |
| posthastectl-dev-only | SHOULD | `posthastectl` remains dev/lab-only until a product CLI/TUI contract is explicitly designed |
| artifact-manifest | SHOULD | Every lab run writes a manifest and summary with reproduction commands and diagnostics |
