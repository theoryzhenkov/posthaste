---
scope: L0
summary: "Autonomous verification lab, headless-first control plane, and agent debugging model"
modified: 2026-05-26
reviewed: 2026-05-26
depends:
  - path: README
  - path: docs/L0-testing
  - path: docs/L0-api
  - path: docs/L0-ui
dependents:
  - path: docs/L1-lab
---

# Lab domain -- L0

## Purpose

PostHaste Lab is the verification and automation control plane for autonomous engineering work. It lets agents and humans launch isolated app profiles, exercise backend and UI behavior, collect structured diagnostics, and iterate without publishing release builds for every investigation.

The lab does not replace product contracts. It orchestrates them. Backend/domain tests, API contracts, frontend state tests, browser automation, and Linux Tauri automation remain separate proof layers with explicit responsibilities.

## Product and control planes

PostHaste has two planes:

- **Product plane**: the daemon, REST/SSE API, web client, desktop client, and future terminal/TUI clients.
- **Control plane**: dev-only launchers, suite registry, fixtures, app drivers, diagnostics, and release-candidate smoke checks.

The product plane must be usable headlessly through the backend API. The control plane may add richer diagnostics, fixture loading, side-effect recorders, and app-driving bridges, but only in explicit lab modes.

## Verification ladder

Verification proceeds from cheap deterministic checks to expensive runtime checks:

1. **Domain/API contracts** prove Rust domain, store, sync, and API behavior directly.
2. **Frontend logic contracts** prove cache invalidation, surface routing, settings state, and Tauri IPC adapters without launching a native shell.
3. **Built-web automation** runs Playwright-style tests against production-built assets and a lab daemon.
4. **Linux Tauri automation** runs a real Tauri app with a feature-gated automation bridge, currently expected to use a `tauri-playwright` spike.
5. **Release artifact smoke** verifies packaged artifacts. Linux can be automated in CI; macOS is manually tested from release builds for now.

macOS automation runners are intentionally out of scope until the Linux/web lab surfaces are reliable.

## Naming model

Lab-visible identifiers follow the operator naming convention:

```text
{type}[:{subtype}].{name}[.{env}[:{machine}]][.{key:value}]*
```

Examples:

```text
suite.api.settings.dev
suite.ui.settings.web.test
suite.desktop.settings.linux.test
fixture.mail.basic.test
profile.lab.empty.dev
runner.tauri-playwright.linux.test
log.backend.jsonl.test
artifact.trace.settings.test
state.app.ready.test
cmd.dev.web.local
cmd.desktop.dev.local
```

The name should be explicit enough for an agent to infer scope, target, and risk without reading implementation files. Command surfaces should be module-oriented, such as `just dev web`, `just web dev`, and `just desktop dev`; do not add flat root aliases when a module command can express the scope.

## Headless-first boundary

Every durable product behavior that the UI depends on should be reachable through the daemon API. The dev-only `posthastectl` can start as an internal lab client for health checks, fixture loading, settings mutation, event waits, and diagnostics. Its stable subset may later become a user-facing headless daemon CLI and a terminal TUI foundation.

## Safety model

The lab has explicit modes:

| Mode | Purpose | Allowed capabilities |
|---|---|---|
| `mode.dev-lab` | Local debugging | Rich diagnostics, disposable profiles, optional real services |
| `mode.ci-lab` | Hermetic CI/agent verification | Disposable roots, no real secrets, structured artifacts |
| `mode.release` | Product/release artifacts | No lab bridge or debug mutation endpoints |

Automation bridges, rich debug endpoints, and side-effect recorders must be disabled unless a lab mode is explicitly selected. Release builds must prove lab-only controls are absent.

## Assertions

| ID | Sev. | Assertion |
|---|---|---|
| lab-orchestrates-contracts | MUST | Lab verification orchestrates existing domain/API/frontend contracts instead of replacing them with broad UI smoke tests |
| headless-api-first | MUST | Product behavior needed by the UI is reachable through daemon APIs so agents can verify it without a graphical client |
| lab-mode-explicit | MUST | Diagnostics, fixture mutation, and app automation bridges require explicit lab mode and are absent in release mode |
| naming-convention | MUST | Lab-visible suite, command, fixture, profile, state, log, runner, and artifact IDs follow `{type}[:{subtype}].{name}[.{env}[:{machine}]][.{key:value}]*` |
| linux-tauri-first | SHOULD | Real desktop automation focuses on Linux Tauri before macOS automation runners are introduced |
| artifact-first-debugging | SHOULD | Lab runs produce structured artifacts that let agents diagnose failures before rerunning blindly |
