---
scope: L0
summary: "Beta telemetry collection for application health, performance, and cache behavior"
modified: 2026-04-28
reviewed: 2026-04-28
depends:
  - path: README
  - path: docs/L0-logging
  - path: docs/L0-api
  - path: docs/L0-accounts
dependents:
  - path: docs/L1-telemetry
---

# Telemetry

PostHaste beta telemetry collects broad product-health signals from consenting beta users. It is separate from local logging. Local logs are for diagnostics on the user's machine; telemetry is a typed, reviewed dataset for aggregate analysis.

## Why

- **Beta readiness**: We need to know whether the app starts, syncs, searches, caches, renders, uploads telemetry, and recovers reliably outside the development environment.
- **Performance**: We need coarse duration and cache-pressure distributions to find slow paths without asking beta users to send logs manually.
- **Product risk**: Mail clients handle highly sensitive data. Telemetry must not become a shadow copy of local logs, mail metadata, search text, or provider traffic.
- **Analysis**: Events need a stable schema and retention plan before upload exists, so later dashboards do not depend on free-form messages.

## Privacy stance

Beta v1 supports two opt-in telemetry modes:

| Mode | Identifier | Answers | Tradeoff |
|------|------------|---------|----------|
| Anonymous aggregate telemetry | None beyond per-event retry dedupe IDs | Health and performance distributions, protocol/message-flow shares, failure buckets | Cannot count unique installs or follow trends for one install |
| Product analytics | Rotating pseudonymous subject ID | Active-install counts, provider share over time, version migration, repeated failure trends | Pseudonymous data; stronger notice, retention, and deletion rules apply |

The product analytics subject ID is not a permanent install ID. It is derived from a local random secret and a short time period, such as month, then rotated. This supports trend analysis while limiting long-term tracking. Pseudonymized data remains personal data when it can be linked back with additional information; masking identifiers is not enough.

The collection philosophy is **wide coverage, narrow fields**. PostHaste should collect every beta-health signal that can pass the privacy rules: explicit event name, low-cardinality enum fields, bucketed counts, bucketed durations, and no user content or stable identity. More event families are acceptable when their fields stay safe and reviewed.

## Scope

In scope:

- Explicit beta opt-in with three user-visible choices: off, anonymous aggregate telemetry, and product analytics
- A typed telemetry event registry distinct from the logging event registry
- On-device event emission for application health, performance, cache, sync, send/receive protocol share, UI, configuration-profile, uploader, and error-bucket signals
- Local telemetry spool under `<data_dir>/telemetry/`, not `<data_dir>/logs/`
- Upload worker with bounded retry, backoff, and queue limits
- PostHaste-owned server-side ingestion endpoint, hosted on the operator's own Hetzner machine for beta v1, with strict schema validation, size limits, rate limits, duplicate handling, and short raw retention
- Aggregate analysis tables and dashboards for beta health
- Public/internal telemetry dictionary listing event names, fields, purpose, sensitivity class, and retention

Out of scope for beta v1:

- Uploading local JSONL logs
- Uploading WebKit console output, crash dumps, raw stack traces, or support bundles
- Session replay, heatmaps, product analytics autocapture, or arbitrary UI event capture
- Permanent install IDs, account IDs, device IDs, advertising IDs, or user IDs
- Per-user behavioral funnels unrelated to product health and provider/protocol planning
- Server-initiated contact, in-app outreach, contact tokens, support prompt rules, or user-provided contact details
- Third-party analytics SDKs embedded in the client
- Required diagnostics that upload without opt-in

## Boundary with logging

Logging and telemetry have opposite defaults. Logs preserve local context so a user or developer can debug one concrete operation. Telemetry removes context so PostHaste can analyze coarse aggregate health without collecting mail data.

The telemetry pipeline must not read, filter, redact, transform, or upload files from `<data_dir>/logs/`. Log event names and local performance fields may inform which telemetry events exist, but telemetry events are emitted separately through telemetry-safe APIs.

## Data flow

```
typed app event
  -> telemetry emitter checks consent and schema
  -> on-device reducer drops or buckets sensitive/high-cardinality values
  -> bounded local spool under <data_dir>/telemetry/
  -> upload worker batches anonymous payloads
  -> HTTPS ingestion endpoint validates and deduplicates
  -> short-retention raw table
  -> aggregate tables with low-count suppression
  -> dashboards and release gates
```

The upload worker must be absent or inert when telemetry is disabled. A fresh install with default settings must make zero telemetry network requests.

## Collection categories

Allowed beta v1 categories:

| Category | Examples | Notes |
|----------|----------|-------|
| App lifecycle | startup, shutdown, foreground/background, update path, crash-recovery marker | No paths, usernames, hostnames, or stack traces |
| Configuration profile | driver-family counts, built-in provider enum, cache-policy flags, enabled-feature enums, data-size buckets | Provider enum is only `fastmail`, `gmail`, `icloud`, `outlook`, `generic`, or `development`; no hostnames, domains, addresses, account IDs, mailbox names, or exact counts |
| Protocol/message flow | received-message buckets by `jmap`/`imap`, sent-message buckets by `jmap_submission`/`smtp` | No message IDs, recipients, sender addresses, subjects, domains, or exact counts |
| Sync health | cycle stage, result, reason bucket, driver family, receive protocol, coarse item-count bucket | No account ID, provider hostname, mailbox name, message ID, state token, or email address |
| Send health | send protocol, result, reason bucket, duration bucket | No recipients, sender address, subject, body, attachment name, or provider host |
| API health | local API duration bucket, status class, route family, payload-size bucket | No request ID, operation ID, path parameters, query text, raw path, or response body |
| Search performance | duration bucket, result-count bucket, query-shape enum, result, reason bucket | No search terms, sender, subject, mailbox name, message IDs, or saved-rule text |
| Cache performance | lookup result, layer, maintenance result, pressure bucket, eviction byte bucket | No cache keys, message IDs, file paths, or exact byte counts |
| UI health | surface enum, command/action enum, render duration bucket, result | No free-form command text, message labels, mailbox labels, or content-derived labels |
| Error buckets | failure reason enum, subsystem enum, recovery result | No error strings, exception text, stack traces, URLs, or provider payloads |
| Uploader health | queue depth bucket, upload result, rejection reason, dropped event count | No payload body echo |

## Banned data

Telemetry must not collect:

- Raw local logs, DEBUG/TRACE logs, WebKit console output, stack traces, SQL, or raw JMAP/IMAP/SMTP payloads
- Email body, subject, snippet, sender, recipients, headers, message IDs, thread IDs, mailbox or folder names, search queries, rules, attachment names, contact data, or address-book data
- Credentials, OAuth tokens, refresh tokens, API keys, cookies, auth headers, session secrets, or passwords
- `account_id`, `request_id`, `operation_id`, `sync_id`, `session_id`, `process_id`, permanent install ID, device ID, advertising ID, MAC address, serial number, or local hostname
- Provider hostnames, target URLs, IP address fields, usernames, email domains, precise locale, precise timezone, precise hardware model, or filesystem paths
- Free-form `message`, `error`, `reason`, `details`, or `exception` strings

## Operational rules

- Telemetry is opt-in and optional.
- Anonymous aggregate telemetry and product analytics are separate user choices. Product analytics must not be enabled as an implicit side effect of aggregate telemetry.
- Opt-out stops collection and upload immediately and deletes the pending local telemetry spool.
- Consent state is stored locally with selected mode, notice version, timestamp, app version, and enabled categories. The consent record is not uploaded as an identifier.
- Product analytics uses a locally stored random secret to derive a rotating subject ID. Opt-out deletes the local secret.
- Local spool retention is at most 72 hours.
- Raw server telemetry retention is at most 30 days during beta.
- Server access/security logs for the ingestion endpoint retain IP/user-agent data for at most 7 days and must not join those fields into analytics tables.
- Aggregate metrics may be retained for 13 months after low-count suppression and removal of join keys.
- Server and client kill switches may reduce or disable telemetry by app version, schema version, or event name. Remote configuration must not add fields or increase collection.

## External guidance

This design follows the conservative parts of current industry guidance:

- [OWASP logging guidance](https://cheatsheetseries.owasp.org/cheatsheets/Logging_Cheat_Sheet.html): exclude sensitive data, validate event data, protect log storage, and handle logging failures.
- [Mozilla data-collection practice](https://firefox-source-docs.mozilla.org/contributing/data-collection.html): review every new collection before it ships.
- [OpenTelemetry data-model practice](https://opentelemetry.io/docs/specs/otel/logs/data-model/): stable event names and typed attributes over message parsing.
- [GDPR privacy principles](https://gdpr-info.eu/art-5-gdpr/): purpose limitation, data minimization, storage limitation, and accountability.
- [NIST de-identification guidance](https://csrc.nist.gov/pubs/sp/800/188/final): treat quasi-identifiers as a re-identification risk, not as solved by redaction alone.

## Assertions

| ID | Sev. | Assertion |
|----|------|-----------|
| default-off | MUST | A fresh install makes no telemetry network requests until the user explicitly opts in |
| separate-pipeline | MUST | Telemetry never uploads, reads, filters, or transforms files from `<data_dir>/logs/` |
| typed-events | MUST | Telemetry events are emitted through a typed registry distinct from logging events |
| anonymous-mode-no-id | MUST | Anonymous aggregate telemetry uploads no stable user, account, device, install, session, operation, request, or process identifier |
| product-mode-explicit | MUST | Product analytics is a separate explicit opt-in and is never enabled implicitly by aggregate telemetry |
| product-mode-rotating-id | MUST | Product analytics uses a rotating pseudonymous subject ID, not a permanent install, account, device, or user ID |
| provider-enum-only | MUST | Provider-level analytics use only the approved enum `fastmail`, `gmail`, `icloud`, `outlook`, `generic`, or `development`; manual/custom providers report `generic` |
| no-contact-path | MUST | Telemetry and product analytics do not provide a way to contact or message users |
| no-mail-data | MUST | Telemetry payloads contain no email content, email metadata, search queries, mailbox names, message IDs, or provider payloads |
| opt-out-delete | MUST | Disabling telemetry immediately stops uploads and deletes the pending local telemetry spool |
| bounded-retention | MUST | Local spool, raw ingest, security logs, and aggregate tables have explicit retention limits |
| reviewed-events | MUST | Every telemetry event has documented owner, purpose, allowed fields, sensitivity class, and retention before implementation |
| broad-safe-coverage | SHOULD | Beta telemetry covers all review-approved health, performance, configuration-profile, cache, sync, UI, and uploader signals that use only privacy-safe fields |
| low-count-suppression | SHOULD | Analytics views suppress cohorts and slices below the configured minimum count |
