# PostHaste telemetry ingest deployment

This directory packages the beta telemetry receiver. It is a remote service for
opt-in telemetry only; it is not the local PostHaste mail API and must not be
used for log upload or support bundles.

## Local smoke

```sh
docker compose -f deploy/telemetry/docker-compose.yml up --build
curl -fsS http://127.0.0.1:8104/healthz
curl -fsS http://127.0.0.1:8104/readyz
```

Submit a sample aggregate batch:

```sh
curl -fsS -X POST http://127.0.0.1:8104/telemetry/v1/batches \
  -H 'content-type: application/json' \
  -H 'authorization: Bearer local-dev-token' \
  --data '{
    "schemaVersion": 1,
    "appVersion": "0.1.0-beta.1",
    "appChannel": "beta",
    "osFamily": "linux",
    "arch": "x86_64",
    "telemetryMode": "aggregate",
    "clientDay": "2026-05-09",
    "events": [{
      "name": "app.startup.completed",
      "version": 1,
      "eventId": "9fb18840-1a4b-4f0a-b94d-9c5e4a8c40c2",
      "fields": {
        "duration_bucket": "s1_5",
        "result": "ok",
        "reason_bucket": "none"
      }
    }]
  }'
```

## Tars fit

The Tars app system in theor-ops expects one app directory with an `app.yaml`.
Use `deploy/telemetry/tars-app.yaml.example` as the starting point:

```text
hosts/nixos/tars/apps/posthaste-telemetry/app.yaml
```

Then run `just app generate` and deploy from theor-ops.

Tars currently generates systemd `docker run` services from YAML. Before beta
traffic is enabled, configure `POSTHASTE_TELEMETRY_INGEST_TOKEN` via app secrets
and add persistent storage for SQLite, equivalent to:

```nix
volumes = [ "/var/lib/posthaste-telemetry:/data" ];
```

If the app generator cannot express that volume yet, extend theor-ops or add a
small Nix override for this app. Do not enable beta telemetry without this
volume, or container replacement will erase all raw and dedupe state.

## Runtime variables

| Variable | Default |
|---|---|
| `POSTHASTE_TELEMETRY_BIND` | `0.0.0.0:8080` |
| `POSTHASTE_TELEMETRY_DATABASE` | `/data/telemetry.sqlite3` |
| `POSTHASTE_TELEMETRY_MAX_BODY_BYTES` | `262144` |
| `POSTHASTE_TELEMETRY_MAX_EVENTS_PER_BATCH` | `100` |
| `POSTHASTE_TELEMETRY_RAW_RETENTION_DAYS` | `30` |
| `POSTHASTE_TELEMETRY_DEDUPE_RETENTION_DAYS` | `7` |
| `POSTHASTE_TELEMETRY_INGEST_TOKEN` | unset; require `Authorization: Bearer <token>` when set |
| `POSTHASTE_TELEMETRY_RATE_LIMIT_PER_MINUTE` | `60` |
| `POSTHASTE_TELEMETRY_DISABLED` | unset; set true for global kill switch |

Local PostHaste builds upload to this service only when configured with:

| Variable | Meaning |
|---|---|
| `POSTHASTE_TELEMETRY_ENDPOINT` | Full HTTPS `/telemetry/v1/batches` URL |
| `POSTHASTE_TELEMETRY_INGEST_TOKEN` | Shared beta token sent as bearer auth |
| `POSTHASTE_TELEMETRY_UPLOAD_INTERVAL` | Upload retry interval, minimum 60 seconds |

## Privacy operations

- Keep reverse-proxy access logs separate from telemetry data.
- Retain access/security logs for at most 7 days.
- Restrict shell/SQLite access to operators.
- Use the global kill switch before deploying a broken client or schema.
