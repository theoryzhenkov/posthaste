---
scope: L1
summary: "Telemetry contracts: consent, event schema, local spool, upload, ingestion, retention, and analysis"
modified: 2026-04-28
reviewed: 2026-04-28
depends:
  - path: docs/L0-telemetry
  - path: docs/L1-logging
  - path: docs/L1-api
  - path: docs/L1-accounts
    section: "TOML schema"
---

# Telemetry -- Contracts

## Architecture

Telemetry is implemented as a first-class subsystem, not as a log exporter.

| Component | Responsibility |
|-----------|----------------|
| Telemetry event registry | Defines event names, versions, field types, purpose, sensitivity class, and retention |
| Client telemetry emitter | Accepts only typed telemetry events and drops or strips fields based on the selected consent mode |
| On-device reducer | Converts measurements to buckets and rejects high-cardinality or banned fields |
| Local spool | Stores anonymous telemetry batches under `<data_dir>/telemetry/` with TTL and byte caps |
| Upload worker | Sends batches with bounded retry, backoff, jitter, and `Retry-After` handling |
| Ingestion endpoint | Validates schema, rejects unknown fields, rate-limits abuse, and writes short-retention raw records |
| Aggregator | Produces low-count-suppressed metric tables for dashboards and release gates |

The logging subsystem remains independent. A log call must never be the input to telemetry upload.

## Consent

Telemetry defaults to disabled. The app presents three user-visible choices:

| Mode | Local value | Upload behavior |
|------|-------------|-----------------|
| Off | `off` | No telemetry collection, no local spool, no telemetry network requests |
| Anonymous aggregate telemetry | `aggregate` | Uploads only anonymous events with no subject ID |
| Product analytics | `product` | Uploads aggregate fields plus approved pseudonymous analytics fields |

The local consent record is stored in app settings:

```toml
[telemetry]
mode = "off"                   # "off", "aggregate", or "product"
notice_version = "2026-04-beta-1"
enabled_at = "2026-04-28T12:00:00Z" # optional, set only after opt-in
categories = ["health", "performance", "cache", "ui", "profile"]
```

The consent record is local configuration. It is not uploaded in telemetry payloads. The app records it so the UI can show the current choice and so support can verify which notice version governed local collection.

Turning telemetry off must:

1. Flip `telemetry.mode` to `off`.
2. Stop accepting new telemetry events.
3. Stop in-flight upload scheduling after the current HTTP request returns or times out.
4. Delete all pending files under `<data_dir>/telemetry/`.
5. Delete the local product analytics secret, if present.

Product analytics is pseudonymous, not anonymous. It may upload a rotating `subjectId` derived from a local random secret and a coarse time period:

```text
subject_id = base64url(HMAC-SHA256(local_product_secret, "posthaste-telemetry:v1:" + YYYY-MM))
```

The raw local secret never leaves the device. The derived subject ID rotates at least monthly. Opting out deletes the secret and pending spool; opting in again creates a new secret.

## Event registry

Telemetry events are registered in source-controlled schema files before use. The implementation may generate Rust and TypeScript types from the registry or maintain parallel typed definitions, but CI must reject events not present in the registry.

Registry entry shape:

```yaml
name: sync.cycle.completed
version: 1
owner: observability
purpose: "Measure beta sync reliability and coarse duration distribution"
category: health
sensitivity: anonymous_aggregate
modes: [aggregate, product]
retention:
  raw_days: 30
  aggregate_months: 13
fields:
  driver_family:
    type: enum
    values: [jmap, imap_smtp, mock]
  duration_bucket:
    type: enum
    values: [lt_1s, s1_5, s5_15, s15_60, m1_5, gt_5m]
  result:
    type: enum
    values: [ok, failed, cancelled]
  reason_bucket:
    type: enum
    values: [none, auth, network, provider_rejected, local_store, timeout, unknown]
```

Events or fields that require `subjectId`, `built_in_provider`, or repeated-install analysis must be marked `modes: [product]` or field-level `mode: product`. The emitter strips product-only fields when the selected mode is `aggregate`.

Allowed field types:

| Type | Use |
|------|-----|
| `bool` | Feature or outcome flags |
| `enum` | Low-cardinality states, categories, or reason buckets |
| `count_bucket` | Rounded counts, not exact user data cardinality |
| `duration_bucket` | Rounded durations |
| `byte_bucket` | Rounded byte sizes |
| `ratio_bucket` | Rounded percentages, such as cache pressure |
| `provider_enum` | Built-in provider category: `fastmail`, `gmail`, `icloud`, `outlook`, `generic`, or `development` |

Disallowed field types:

- Free-form strings
- Raw integers that count user content exactly, unless bucketed before upload
- Floats with high precision
- Timestamps other than coarse client day or server receive time
- Arrays except fixed enum sets explicitly approved in the registry
- Nested objects outside the fixed payload envelope

## Payload envelope

The uploader sends batches of telemetry events. Every field is allowlisted and versioned.

```json
{
  "schemaVersion": 1,
  "appVersion": "0.1.0-beta.1",
  "appChannel": "beta",
  "osFamily": "macos",
  "arch": "aarch64",
  "telemetryMode": "aggregate",
  "clientDay": "2026-04-28",
  "events": [
    {
      "name": "sync.cycle.completed",
      "version": 1,
      "eventId": "9fb18840-1a4b-4f0a-b94d-9c5e4a8c40c2",
      "fields": {
        "driverFamily": "jmap",
        "durationBucket": "s5_15",
        "result": "ok",
        "reasonBucket": "none"
      }
    }
  ]
}
```

`eventId` is a random per-event dedupe key generated when the event is written to the spool. It is retained server-side only long enough to reject retries and must not be used as a client identity.

When `telemetryMode` is `product`, the envelope may also include:

```json
{ "subjectId": "rotating-monthly-hmac-id" }
```

`subjectId` is banned in aggregate mode. It must not be used to store user-facing identity, account identity, mail identity, or support identity.

Neither consent mode supports user contact. The client does not upload contact details, the server does not maintain contact tokens, and product analytics subject IDs must not be used for in-app outreach or support prompts.

Envelope limits:

| Limit | Value |
|-------|-------|
| Max compressed request size | 64 KiB |
| Max decompressed request size | 256 KiB |
| Max events per request | 100 |
| Max fields per event | 16 |
| Max event age at upload | 72 hours |
| Max enum/string field length | 64 ASCII bytes |

`appVersion`, `appChannel`, `osFamily`, and `arch` are coarse environment fields. The client must not upload exact OS patch version, timezone, locale, hardware model, hostname, username, screen resolution, or user agent.

## Event coverage

Beta v1 is coverage-first within the privacy boundary. It should instrument every useful beta-health path whose fields can be expressed as enums, booleans, and buckets. Adding more safe events is preferred over relying on local logs or free-form feedback.

Initial event families:

| Family | Event examples | Purpose |
|--------|----------------|---------|
| App lifecycle | `app.startup.completed`, `app.shutdown.completed`, `app.recovery.completed`, `app.update.detected` | Startup, shutdown, recovery, and update health |
| Configuration profile | `profile.snapshot.recorded`, `profile.feature.enabled`, `profile.cache_policy.recorded` | Understand coarse beta configurations and product adoption |
| Provider/protocol profile | `profile.provider.recorded`, `profile.protocol_flow.recorded` | Measure built-in provider share and JMAP/IMAP/SMTP flow share over time |
| Account/driver setup | `account.setup.completed`, `account.verify.completed`, `driver.capability.detected` | Setup reliability by provider, driver, auth, and capability family |
| Sync | `sync.cycle.completed`, `sync.stage.completed`, `sync.push.completed` | Sync and push reliability by stage and reason bucket |
| Send | `send.message.completed`, `send.submission.completed` | Send reliability by submission protocol |
| Local API | `api.request.completed`, `api.sse.completed` | Local API and event-stream health by route family |
| Search | `search.query.completed`, `search.preview.completed`, `search.rule_evaluation.completed` | Search performance and rule cost without query text |
| Cache | `cache.lookup.completed`, `cache.write.completed`, `cache.maintenance.completed`, `cache.eviction.completed` | Cache effectiveness, pressure, and maintenance cost |
| Store/runtime | `store.operation.completed`, `runtime.task.completed`, `runtime.queue.recorded` | Local store and background task health |
| UI | `ui.surface.opened`, `ui.command.completed`, `ui.render.completed`, `ui.error_boundary.triggered` | Coarse UI usage and render reliability |
| Telemetry | `telemetry.upload.completed`, `telemetry.event.dropped`, `telemetry.schema.rejected` | Telemetry pipeline health |

Initial event field shapes:

| Event | Fields | Purpose |
|-------|--------|---------|
| `app.startup.completed` | `duration_bucket`, `result`, `reason_bucket` | Startup reliability |
| `profile.snapshot.recorded` | `driver_family_set`, `account_count_bucket`, `mailbox_count_bucket`, `local_message_count_bucket`, `cache_policy`, `enabled_feature_set` | Coarse configuration profile |
| `profile.provider.recorded` | `built_in_provider`, `driver_family`, `auth_family`, `account_count_bucket` | Built-in provider share over time |
| `profile.protocol_flow.recorded` | `receive_protocol`, `send_protocol`, `received_count_bucket`, `sent_count_bucket` | Protocol share for received and sent mail |
| `account.setup.completed` | `driver_family`, `auth_family`, `built_in_provider`, `result`, `reason_bucket` | Account setup reliability |
| `account.verify.completed` | `driver_family`, `auth_family`, `built_in_provider`, `result`, `reason_bucket` | Provider verification reliability |
| `driver.capability.detected` | `driver_family`, `capability`, `availability` | Capability coverage without provider identity |
| `sync.cycle.completed` | `driver_family`, `receive_protocol`, `trigger`, `duration_bucket`, `result`, `reason_bucket`, `item_count_bucket` | Sync reliability and received-mail protocol share |
| `sync.stage.completed` | `driver_family`, `stage`, `duration_bucket`, `result`, `reason_bucket`, `item_count_bucket` | Locate slow/failing sync phases |
| `sync.push.completed` | `driver_family`, `transport_family`, `duration_bucket`, `result`, `reason_bucket` | Push reliability |
| `send.message.completed` | `send_protocol`, `driver_family`, `duration_bucket`, `result`, `reason_bucket`, `recipient_count_bucket`, `attachment_count_bucket` | Send reliability and sent-mail protocol share |
| `send.submission.completed` | `send_protocol`, `driver_family`, `duration_bucket`, `result`, `reason_bucket` | Submission transport reliability |
| `api.request.completed` | `route_family`, `method_family`, `duration_bucket`, `status_class`, `payload_size_bucket`, `result` | Local API health |
| `api.sse.completed` | `duration_bucket`, `event_count_bucket`, `result`, `reason_bucket` | Event stream reliability |
| `search.query.completed` | `duration_bucket`, `result_count_bucket`, `query_shape`, `operator_set`, `result`, `reason_bucket` | Search performance without query text |
| `search.preview.completed` | `duration_bucket`, `result_count_bucket`, `query_shape`, `result`, `reason_bucket` | Preview cost |
| `search.rule_evaluation.completed` | `duration_bucket`, `rule_shape`, `candidate_count_bucket`, `result`, `reason_bucket` | Smart mailbox and automation rule cost |
| `cache.lookup.completed` | `cache_layer`, `result`, `duration_bucket`, `size_bucket` | Cache hit/miss and cost |
| `cache.write.completed` | `cache_layer`, `duration_bucket`, `size_bucket`, `result`, `reason_bucket` | Cache write cost |
| `cache.maintenance.completed` | `duration_bucket`, `pressure_bucket`, `evicted_bytes_bucket`, `result` | Cache pressure and maintenance behavior |
| `cache.eviction.completed` | `cache_layer`, `evicted_bytes_bucket`, `evicted_item_count_bucket`, `reason_bucket` | Eviction behavior |
| `store.operation.completed` | `operation_family`, `duration_bucket`, `result`, `reason_bucket` | SQLite/store health |
| `runtime.task.completed` | `task_family`, `duration_bucket`, `result`, `reason_bucket` | Background task health |
| `runtime.queue.recorded` | `queue_family`, `depth_bucket`, `pressure_bucket` | Backpressure and queue health |
| `ui.surface.opened` | `surface`, `result` | Coarse UI route usage |
| `ui.command.completed` | `command_family`, `surface`, `duration_bucket`, `result`, `reason_bucket` | Command reliability |
| `ui.render.completed` | `surface`, `render_duration_bucket`, `item_count_bucket`, `result` | Render cost |
| `ui.error_boundary.triggered` | `surface`, `reason_bucket`, `recovery_result` | Frontend failure buckets without console text |
| `telemetry.upload.completed` | `batch_size_bucket`, `result`, `reason_bucket` | Telemetry pipeline health |
| `telemetry.event.dropped` | `drop_reason`, `count_bucket` | Detect local schema, quota, or consent drops |
| `telemetry.schema.rejected` | `event_family`, `reject_reason`, `count_bucket` | Detect client/server schema drift |

Enums are intentionally coarse. `built_in_provider` is product-analytics-only and is limited to `fastmail`, `gmail`, `icloud`, `outlook`, `generic`, or `development`. Built-in setup flows may emit their provider enum. Manual/custom providers must emit `generic`; the client must never derive a provider enum from hostnames, domains, MX records, email addresses, or URLs. Route families and surfaces are explicit enums. API path parameters and query strings are never uploaded. Query shapes describe parser structure only, such as `simple_term`, `field_filter`, `boolean`, or `advanced`, never the terms, field values, sender, subject, mailbox name, or saved-rule text.

## Banned fields

The registry, client reducer, and server validator must reject fields whose names or values match banned classes:

| Class | Examples |
|-------|----------|
| Identity and correlation IDs | `account_id`, `request_id`, `operation_id`, `sync_id`, `session_id`, `process_id`, permanent install ID |
| Mail data | subject, sender, recipient, email address, domain, message ID, thread ID, mailbox name, search query |
| Network targets | URL, host, IP address, provider origin, JMAP session URL, IMAP host, SMTP host, MX domain |
| Secrets | token, password, cookie, auth header, OAuth credential, API key |
| Local machine data | username, hostname, filesystem path, exact locale/timezone, hardware model |
| Free text | `message`, `error`, `exception`, `details`, stack trace, console output |

The validator must inspect both field names and string-like field values. Banned-value checks are a backstop; the schema should make such values unrepresentable.

`subjectId` is permitted only in the payload envelope when `telemetryMode = "product"`. It is never permitted as an event field.

## Local spool

The local spool lives under `<data_dir>/telemetry/`.

Rules:

- Directory permissions are owner-only where the platform supports it.
- Files are written atomically: write temp file, fsync when feasible, rename into place.
- File names contain no user data: `batch-<uuid>.json`.
- Total spool size is capped at 1 MiB for beta v1.
- Files older than 72 hours are deleted before upload.
- If the cap is reached, new events are dropped and `telemetry.event.dropped` records a bucketed count when possible.
- Startup must not block on upload. Spool cleanup may run asynchronously after config load.

The spool format is the same payload envelope the uploader sends, except it may contain a local-only `created_at` field used for TTL. That field is stripped before upload.

## Upload

The uploader uses HTTPS `POST` to the telemetry ingestion endpoint with `Content-Type: application/json`. It sends no cookies, mail auth headers, account credentials, or bearer token from the mail client. Responses use `Cache-Control: no-store`.

Retry rules:

- Retry only transient network errors, `429`, and `503`.
- Honor `Retry-After` when present.
- Otherwise use exponential backoff with jitter.
- Do not retry forever in a tight loop; each batch has a retry budget.
- On `400`, `413`, `415`, or schema rejection, delete the batch and record a local dropped-event count.
- Opt-out cancels future scheduling and deletes pending batches.

The client may include a generic beta app token only if needed to distinguish official builds from arbitrary traffic. That token must not identify a user, install, account, or device.

## Ingestion endpoint

The endpoint is not part of the local app API at `/v1`. It is a remote service endpoint owned by the release infrastructure. Beta v1 targets a PostHaste-owned service deployed on the operator's Hetzner machine.

Server validation:

- Accept only `POST`.
- Require `Content-Type: application/json`.
- Enforce compressed and decompressed body limits before parsing nested content.
- Reject unknown top-level fields, unknown event names, unsupported versions, unknown fields, invalid enum values, excessive nesting, oversized arrays, and excessive field lengths.
- Reject payloads containing banned field names or banned value patterns.
- Return generic client errors that do not echo payload contents.
- Rate-limit by coarse network source and deployment token where available.
- Deduplicate `eventId` values for a short retention window.

Server storage:

- Raw payloads land in a short-retention table with server receive time.
- IP address, user agent, CDN request ID, and TLS metadata remain only in separate security/access logs with at most 7-day retention.
- Analytics tables do not contain IP address, user agent, request IDs, or raw payload JSON.
- Raw access requires least privilege and audit logging.

## Analysis

Dashboards and release gates read aggregate tables, not raw payloads.

Required dashboards:

- Startup success rate and duration distribution by app version and OS family
- Active opt-in product-analytics installs by app version and OS family
- Built-in provider share over time, with `generic` representing all manual/custom providers
- Received-mail protocol share over time: JMAP vs IMAP
- Sent-mail protocol share over time: JMAP submission vs SMTP
- Sync success rate and reason buckets by driver family
- Local API duration distribution by route family and status class
- Search duration and result-count distributions
- Cache hit/miss, pressure, eviction, and maintenance duration
- Telemetry ingestion volume, rejection rate, dropped local event count, upload failure rate, and endpoint `4xx`/`5xx`

Aggregate queries must suppress low-count slices. Beta v1 uses `n < 20` as the default suppression threshold unless a stricter threshold is configured for rare events.

## Development and release workflow

Adding or changing a telemetry event requires:

1. Add or update the registry entry.
2. Document owner, purpose, fields, sensitivity class, and retention.
3. Add client type coverage.
4. Add server schema coverage.
5. Add fixture tests proving banned local log content cannot reach telemetry.
6. Update the public beta telemetry dictionary.
7. Run privacy/security review before shipping.

The release checklist must include a telemetry review item. A beta cannot ship with an event registry change that lacks tests and dictionary documentation.

## Assertions

| ID | Sev. | Assertion |
|----|------|-----------|
| consent-default-disabled | MUST | Telemetry consent defaults to disabled in local settings |
| consent-modes | MUST | Telemetry consent exposes `off`, `aggregate`, and `product` modes as explicit user choices |
| consent-local-only | MUST | Consent timestamp, notice version, app version, selected mode, and categories are stored locally and not uploaded as identifiers |
| opt-out-purge | MUST | Disabling telemetry stops collection/upload, deletes pending spool files, and deletes the local product analytics secret |
| aggregate-no-subject | MUST | Aggregate telemetry payloads never include `subjectId` or any stable client identifier |
| product-rotating-subject | MUST | Product analytics uses a rotating pseudonymous `subjectId` derived from a local secret and coarse time period |
| provider-enum | MUST | Provider analytics uses only `fastmail`, `gmail`, `icloud`, `outlook`, `generic`, or `development`; manual/custom providers always emit `generic` |
| provider-product-only | MUST | Built-in provider enum fields are uploaded only in product analytics mode |
| protocol-flow | MUST | Telemetry can report received-message protocol share (`jmap` vs `imap`) and sent-message protocol share (`jmap_submission` vs `smtp`) using bucketed counts |
| no-contact | MUST | Telemetry and product analytics do not upload contact details, maintain contact tokens, or support server-initiated user outreach |
| registry-required | MUST | Client and server builds reject telemetry event names not present in the registry |
| registry-metadata | MUST | Every registry event records owner, purpose, category, sensitivity, field definitions, and retention |
| field-type-allowlist | MUST | Telemetry fields use only allowed low-cardinality, bucketed, or boolean types |
| banned-field-reject | MUST | Registry, client, and server checks reject banned identity, mail, network-target, secret, local-machine, and free-text fields |
| payload-limits | MUST | The ingestion endpoint enforces body size, event count, field count, field length, and nesting limits |
| per-event-dedupe | MUST | Random per-event IDs deduplicate retries without becoming a long-lived client identifier |
| spool-separated | MUST | Telemetry uses `<data_dir>/telemetry/` and never `<data_dir>/logs/` |
| spool-bounded | MUST | Local spool size, event age, startup impact, and retry behavior are bounded |
| upload-no-credentials | MUST | Telemetry uploads include no cookies, mail auth headers, account credentials, or mail-client bearer tokens |
| retry-backoff | MUST | Upload retries honor `Retry-After` and otherwise use backoff with jitter and a retry budget |
| ingest-generic-errors | SHOULD | Ingestion errors do not echo rejected payload contents |
| raw-retention | MUST | Raw telemetry records are retained for at most 30 days in beta |
| security-log-separation | MUST | Ingestion IP/user-agent/security metadata is kept out of analytics tables and retained for at most 7 days |
| aggregate-suppression | SHOULD | Analytics suppress slices below the low-count threshold |
| dictionary-current | MUST | The beta telemetry dictionary matches the shipped event registry |
| fixture-sensitive-logs | MUST | Tests prove representative logs containing tokens, URLs, email addresses, subjects, message IDs, and TRACE payloads do not produce sensitive telemetry |
