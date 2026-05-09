---
scope: L2
summary: "Dockerized beta telemetry ingestion service for Tars deployment"
modified: 2026-05-16
reviewed: 2026-05-16
depends:
  - path: docs/L0-telemetry
  - path: docs/L1-telemetry
---

# Telemetry ingest service -- L2

## Purpose

`posthaste-telemetry-ingest` is the remote receiver for opt-in beta telemetry.
It is separate from the local PostHaste mail API and never reads local log files.
The first deployment target is the Tars app host, where apps are Docker images
managed by NixOS/systemd and published through nginx + ACME.

## Service boundary

The service exposes only:

| Method | Path | Purpose |
|---|---|---|
| `POST` | `/telemetry/v1/batches` | Accept one telemetry batch |
| `GET` | `/healthz` | Process liveness |
| `GET` | `/readyz` | SQLite readiness |

The local desktop/browser app posts directly to the HTTPS endpoint after the user
opts in. The endpoint is not part of the local `/v1` app API.

## Local client spool

The local mail app uses `crates/posthaste-telemetry` to emit typed telemetry
batches under `<state_root>/telemetry/pending/`. When `telemetry.mode = "off"`,
the emitter returns before creating the telemetry directory. Opting out through
`PATCH /v1/settings` normalizes local consent to `off` and deletes the telemetry
root, including pending batches and the product-mode secret.

Local spool rules:

- pending batches are JSON payloads compatible with `/telemetry/v1/batches`
- owner-only permissions are applied on Unix (`0700` dirs, `0600` files)
- writes are atomic temp-file writes followed by `fsync` and rename
- files older than 72 hours are removed during emission
- the pending spool is capped at 1 MiB; further events are dropped when full
- product mode derives a monthly pseudonymous `subjectId` from a local secret

The local upload worker starts only when `POSTHASTE_TELEMETRY_ENDPOINT` is set.
Before each upload attempt it rereads app settings and skips all network I/O when
`telemetry.mode = "off"`. Successful uploads delete pending batch files. Invalid
payload responses (`400`, `413`, `415`, `422`) discard the affected batch; rate
limits and server/network failures retain it for retry.

## Runtime

The remote binary is `posthaste-telemetry-ingest` from
`crates/posthaste-telemetry-ingest`. Runtime configuration is environment-based:

| Variable | Default | Meaning |
|---|---|---|
| `POSTHASTE_TELEMETRY_BIND` | `127.0.0.1:8080` | Bind address; Docker image overrides to `0.0.0.0:8080` |
| `POSTHASTE_TELEMETRY_DATABASE` | `/data/telemetry.sqlite3` | SQLite database path |
| `POSTHASTE_TELEMETRY_MAX_BODY_BYTES` | `262144` | Max JSON request body |
| `POSTHASTE_TELEMETRY_MAX_EVENTS_PER_BATCH` | `100` | Max events per request |
| `POSTHASTE_TELEMETRY_RAW_RETENTION_DAYS` | `30` | Raw batch/event retention |
| `POSTHASTE_TELEMETRY_DEDUPE_RETENTION_DAYS` | `7` | Retry dedupe retention |
| `POSTHASTE_TELEMETRY_INGEST_TOKEN` | required | Shared beta build token required as `Authorization: Bearer` |
| `POSTHASTE_TELEMETRY_RATE_LIMIT_PER_MINUTE` | `60` | Process-wide ingestion request limit |
| `POSTHASTE_TELEMETRY_DISABLED` | unset | Return 503 from ingestion when true |

Set `POSTHASTE_TELEMETRY_ALLOW_UNAUTHENTICATED=true` only for isolated local
manual testing; public deployments must use an ingest token.

Local mail-app upload worker variables:

| Variable | Default | Meaning |
|---|---|---|
| `POSTHASTE_TELEMETRY_ENDPOINT` | unset | Full HTTPS remote `/telemetry/v1/batches` URL; localhost HTTP is allowed for tests; unset disables uploads |
| `POSTHASTE_TELEMETRY_INGEST_TOKEN` | unset | Bearer token sent by the local uploader |
| `POSTHASTE_TELEMETRY_UPLOAD_INTERVAL` | `300` | Upload scan interval in seconds, minimum 60 |

The service stores raw event rows and retry dedupe IDs in SQLite. It applies
retention on startup. A later worker may run retention periodically, but beta v1
can rely on service restarts plus operator-triggered restarts until upload volume
requires hourly cleanup.

## Validation

The server validates the envelope and event fields before storage:

- `schemaVersion` must be `1`.
- `telemetryMode = aggregate` must not include `subjectId`.
- `telemetryMode = product` must include only a short safe `subjectId`.
- Unknown top-level fields are rejected by JSON deserialization.
- Unknown event names, versions, fields, and enum values are rejected.
- Event IDs must parse as UUIDs and are used only for retry dedupe.
- String enum values must be short ASCII tokens and are scanned for banned
  sensitive-value shapes.

The source-controlled registry lives at `docs/telemetry/events.yaml`, with a
matching Rust allowlist in `crates/posthaste-telemetry-ingest/src/registry.rs`.
Changing telemetry collection requires updating both and adding tests.

## Tars deployment fit

Tars apps are declared under `hosts/nixos/tars/apps/<app>/app.yaml` in
theor-ops. For this service, the app should use the standard web-app shape:

```yaml
app: posthaste-telemetry
image: ghcr.io/theoryzhenkov/posthaste/posthaste-telemetry-ingest:latest
containerPort: 8080
hostPort: 8104
domain: telemetry.theor.net
```

The generated NixOS app module runs the container under systemd with
`docker run`, maps `127.0.0.1:<hostPort>` to the container, and nginx terminates
TLS for `domain`. The deployment still needs a shared ingest token in app secrets
and a persistent volume entry before beta traffic is enabled. If the current Tars
app generator cannot express the volume, either extend theor-ops app generation
or add a small Nix override for this app:

```nix
volumes = [ "/var/lib/posthaste-telemetry:/data" ];
```

## Docker image

`deploy/telemetry/Dockerfile` builds the Rust binary and runs it as a non-root
user with `/data` as the SQLite volume. The image does not include client mail
app assets and does not need PostHaste account configuration.

## Privacy operations

- The reverse proxy may keep IP/user-agent access logs for security, but those
  logs are not joined into telemetry tables and must retain for at most 7 days.
- Routine analysis should query aggregate views once they exist, not raw rows.
- Raw SQLite access on Tars should be limited to operators.
- The global kill switch is `POSTHASTE_TELEMETRY_DISABLED=true` plus an app
  restart.

## Assertions

| ID | Sev. | Assertion |
|----|------|-----------|
| remote-service-separate | MUST | The telemetry receiver is a separate remote service and is not mounted under the local mail app `/v1` API |
| no-log-source | MUST | The ingest service never accepts local log files, console logs, stack traces, or support bundles as telemetry payloads |
| validate-before-store | MUST | Unknown event names, fields, versions, and invalid enum values are rejected before SQLite writes |
| aggregate-no-subject | MUST | Aggregate-mode payloads containing `subjectId` are rejected |
| retry-dedupe-only | MUST | `eventId` is used only for retry dedupe and is not treated as an install, session, account, or user identity |
| tars-volume-required | MUST | Tars deployment mounts persistent storage at `/data` before beta telemetry is enabled |
| beta-token-required | MUST | Public beta deployment configures a shared ingest token and rejects requests without `Authorization: Bearer` |
