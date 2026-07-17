---
title: "Backend (L1)"
scope: L1
summary: "The backend's internal structure: the domain service over the SQLite store, provider gateways, per-account runtimes, the outbox, events and the store generation, query evaluation, and command execution."
modified: 2026-07-17
reviewed: 2026-07-17
state: draft
depends:
  - path: docs/architecture/L1-architecture
  - path: docs/api/L1-api
  - path: docs/state/mail/L1
dependents:
---

# Backend

## 1. Boundary

The backend is one Rust process built around the domain core. It owns the
SQLite store, runs provider sync and automation, executes commands, and
serves the API on a loopback port. The Tauri app embeds it; the same binary
runs standalone for development and headless use. Everything below the API
is internal: consumers see queries, commands, the event stream, and blobs —
never the store, the gateways, or the runtimes.

## 2. Domain service

The domain service is the center of the backend and the only component that
touches the store. It applies commands, runs the sync cycle, drives the
outbox, evaluates automation rules, and emits domain events. Every other
component is an adapter around it: the API layer translates HTTP to service
calls, gateways translate provider protocols to the domain model, account
runtimes decide when the service syncs. Adapters own transport and
scheduling; the service owns state and transactions. This seam is what makes
the backend testable — the service runs complete against in-memory ports,
and the same tests drive it against a real provider.

## 3. Provider gateways

One gateway contract abstracts the providers: JMAP (over the engine) and
IMAP/SMTP implement the same interface, normalizing provider objects into
the domain model — messages, mailboxes, stable ids — so the service is
provider-agnostic. Push inputs (JMAP EventSource, IMAP IDLE) surface as sync
triggers. Every provider call goes through the call-policy envelope: a
per-class deadline, typed failure classification, and the send-class rule
that a timeout after dispatch is *uncertain*, parked rather than retried —
the exactly-once discipline lives at this seam.

## 4. The store

One SQLite database holds three planes: base (provider truth), the overlay
(folded effect of pending intents), and the outbox (the intents themselves).
The ownership rules are absolute: sync writes base and nothing else does —
enforced at compile time by a write-capability type — the command fold
writes the overlay, and nothing ever copies overlay state into base; when a
provider confirms an intent, sync writes the resulting truth into base and
the overlay entry retires. All reads go through effective views that serve
base + overlay already folded. The schema carries a version; migrations run
forward on open, and an older binary refuses a newer database cleanly. The
full state model — projections, freshness, query semantics — is specified in
the mail-state docs; this document's contract is the ownership rules above.

## 5. Account runtimes

Each enabled account runs under a supervisor that owns its provider
connections and decides when it syncs: on push, on an interval fallback, on
explicit request, and after mutations flush. The supervisor keeps the push
connection alive with backoff and reconnect, reports account health as
queryable status, and recovers from provider failures by degrading to
polling rather than stopping. Enabling, disabling, or re-crediting an
account restarts its runtime; no other component notices.

## 6. The outbox

Mail mutations live as typed intents with a full lifecycle. Acceptance
enqueues the intent and folds its effect into the overlay in one
transaction — the user-visible optimism and the durable record are the same
write. A flush pass pushes ready intents through the gateway; settlement
records the verdict. Confirmed intents retire when sync lands the provider
truth in base; uncertain dispatches park for explicit resolution rather than
blind retry; permanent failures keep their verdict as queryable state. The
intent id is the idempotency key end to end — retries at every layer, up to
provider-visible send identity, deduplicate against it. Readiness gates
scheduled work: undo-send holds on the monotonic clock, send-later on wall
time, so clock skew can never fire a send early or strand it.

## 7. Events and the generation

Every committed write emits domain events on an in-process bus and advances
the store generation — an atomic counter scoped to one backend run, paired
with a run id minted at startup. The SSE adapter subscribes to the bus and
broadcasts to all connected clients, coalescing bursts so a sync storm
becomes a handful of messages; a generation-only heartbeat covers silence.
Query answers are stamped with the generation observed *before* evaluation:
if a write lands mid-read, the stamp is conservative and the next stream
message triggers a refetch — staleness always resolves toward a refetch,
never toward trusting an old answer.

## 8. Query evaluation

The API's typed queries map one-to-one onto reads over the effective views:
mail lists and search (the query grammar compiled to SQL, including FTS),
threads, message details, mailbox counts (maintained counters, not scans),
accounts and their health, and pending operations (the outbox with
verdicts). List queries are windowed by limit and keyset cursor and return a
screenful bounded by the window, never by mailbox size. There is no query
cache and no materialized per-client state: every answer is evaluated fresh
from the store, stamped with the generation, and forgotten.

## 9. Command execution

A command arrives as a typed envelope with a client-generated id. Execution
is: decode to the intent vocabulary, check the caller's capability set,
apply, reply with the resulting generation. Mail mutations apply through the
outbox path (§6); configuration commands — accounts, settings, rules —
apply directly in the service. Replaying an id returns the original
outcome. The reply means "recorded and visible at this generation";
provider-side results arrive later as verdict state, never as a late HTTP
response.

## 10. Config, secrets, lifecycle

Configuration is file-based in the app data directory; secrets live in the
OS keychain behind a secret-store port. Startup order: open and migrate the
store, start the domain service, start account runtimes, bind the API port,
write the connection-info file. Shutdown reverses it: stop intake, stop
runtimes, flush, remove the connection info. A watchdog observes sync loops
and stream health and restarts stalled account runtimes; the API staying up
while an account degrades is the designed failure mode — clients keep
reading state, and account health is itself a query.
