# RFC-L2: The mirror client — one evaluator, surfaces as state, commands as intents

Status: **PROPOSED** (drafted 2026-07-16, awaiting owner ratification).
Parent: RFC-L2-client-replication-model — NS1's sealed-base/overlay/effective
substrate is what makes this model possible. Supersedes (on ratification):
RFC-L2-view-membership-negotiation (pre-implementation), the pending tail of
RFC-L2-client-resilience (M41–M45, M49–M50), PLAN-L2-client-link-unification,
and docs/issues/L2-single-source-view-membership.

## 1. Problem

Posthaste is ~180k LOC. Measured by crate/directory, roughly **~50k of it is
the seam** that exists only because the UI and the data live behind a
replication boundary: the six link/replica crates (link-near-end, link-far-end,
contract-core's frame vocabulary, authority-server-link, replica-projector,
replica-core, client-node-wasm ≈ 13k), the runtime's far-end view machinery
(~9k), http-api-adapter's stream/link surface (part of 14k), the web app's
`runtime/` + `domain-cache/` + connection layers (~10k), two schema codegen
pipelines, and the seam-shaped share of the server test suites.

The deeper cost is structural. The client is a **second replica with its own
logic**: it holds partial state, evaluates membership predicates, folds its own
optimism, and reconciles the runtime's re-serves against its local state. Every
hard client bug of the L2 era is a *disagreement between two evaluators* — the
move/delete flicker, the option-iii staleness class, the stale-stamp failure
the L3 convergence test exposed, BE-H3's hanging send promise — and three of
the four open roadmap fronts (client-resilience, link-unification,
view-membership negotiation) are rent paid to keep the two evaluators agreeing.
Users feel the rent as the version-skew update model (7+ artifact families
against one one-way-migrating DB), as staleness bugs, and as feature velocity
lost to seam maintenance.

Meanwhile NS1/NS2 quietly built the other half of a much simpler system: the
daemon's store now materializes `visible = fold(base, overlay)` **in the store
itself** (`_effective` views), optimism is invisible below the read boundary,
pending work is a typed-intent outbox, and verdicts are projections. The
backend already produces exactly the thing a dumb client would want to read.

## 2. The model

- The **backend** (Rust) is the sole evaluator. It syncs providers, folds
  optimism (NS1 overlay), and **materializes surfaces**: named, windowed
  view-states for whatever the client currently displays (inbox-list window,
  open thread, sidebar counts, search results, outbox/verdict pane). All open
  surfaces of a session form one versioned **state document**.
- The **client** (TypeScript) is a mirror: subscribe to the patch stream, apply
  patches to one local state store, render it, and send **commands** (the NS2
  typed intents) back. It evaluates nothing, folds nothing, caches nothing
  beyond the document, and cannot disagree with the backend about anything.
- **Optimism is backend-only and invisible.** A command folds into the overlay
  and the resulting patch arrives on the same stream as any other change; the
  client does not know (or care) whether a row it renders is provider-confirmed
  or overlay-pending. The pending-ops ledger (the outbox) is separate state,
  rendered — like everything else — as a surface.

This is the state-mirror shape (cf. earendil-works/pi's coding-agent package):
the client is plain, readable TypeScript a user can modify in place;
integrations need to understand exactly two things — *read the state document,
send commands* — never replication.

## 3. Decisions

- **D180 — One evaluator.** All view state is materialized backend-side from
  the effective views. The client-replica machinery — client predicates,
  entity-store folding, re-serve reconciliation, membership negotiation — is
  retired wholesale, not renegotiated. The disagreement bug class becomes
  unrepresentable rather than better-managed.

- **D181 — Surfaces as state.** The unit of client-visible state is the
  surface: `(kind, scope, window)` materialized to rows/values. Opening,
  closing, and window-extension are commands; content is the backend's job.
  Surfaces replace view descriptors, coverage/watermark negotiation, and the
  per-view event pumps. Scrolling = an extend-window command answered by a
  patch (the runtime's coverage model, minus the client second-guessing it).
  Compose buffers and other ephemeral editor state stay client-local — a text
  editor is not a replica.

- **D182 — Patch protocol with trivial recovery.** The session document is a
  snapshot plus seq-numbered patches. Any gap, reconnect, or doubt → the client
  requests the full document, which is screen-sized by construction (surfaces,
  not mailboxes). This deletes the collapse/gap-detection machinery, the client
  half of the mutation dedup ledger, and the replay window as protocol
  concepts; "resync" stops being a hard path because it is the same code as
  "connect".

- **D183 — The recomputer: dirty → coalesce → diff.** Domain events mark
  surfaces dirty; a per-tick coalescing recomputer rebuilds dirty surfaces from
  the effective views, diffs against the last shipped state, and emits minimal
  patches. Never per-event naive recompute — the O(views × rows) storm that
  spawned option iii is the cautionary tale, now solved as a *local*
  engineering problem (dirty-tracking sits directly on the store, no protocol
  in the loop) instead of a distributed one. This is the one genuinely new
  backend component; everything it needs (event stream, `build_snapshot`-class
  query code, projection tracking) already exists.

- **D184 — Commands are NS2 intents; one write path for every caller.** UI,
  `posthastectl`, MCP agents, and user scripts all speak the same command API
  (typed intents, D155 envelopes, idempotent by op id). A command returns an
  acceptance receipt; its *effects* arrive as surface patches, and its
  *verdict* is state on the outbox surface (verdict-as-projection, D154 —
  already true). BE-H3's failure mode (a client promise awaiting a daemon
  that restarted) dissolves: there is no promise to hang — state either shows
  the op pending, settled, or failed.

- **D185 — Perceived latency is backend optimism plus local IPC.** The UX
  budget for an action is command → overlay fold → patch, sub-millisecond
  in-process/local-socket. Client-side optimism is retired everywhere; if a
  surface ever feels slow, the fix is in the recomputer's tick, not in a
  client-side speculation layer.

- **D186 — The hackable client.** The TS client is a plain state store + a
  fetch wrapper, readable and modifiable in place; one schema codegen pipeline
  (commands + surface types) instead of two. The scripting/MCP surface and the
  UI stop being separate integration models — a user extension is
  architecturally identical to a first-party feature.

- **D187 — Local-first, remote-capable; process unification is a follow-on,
  not a prerequisite.** The protocol stays a socket protocol (local socket /
  HTTP+SSE), so a remote daemon with an attached browser remains *possible*
  (LiveView-shaped: backend-owned windowing tolerates latency) but no longer
  drives the architecture. The desktop app may keep the daemon as a managed
  sidecar initially — the simplification is the client/protocol model, not the
  process count. Artifact/update-model consolidation is Slice 5, deliberately
  last and severable.

## 4. What dies, what survives

**Dies** (≈40–50k LOC + three roadmap fronts): runtime far-end view registry /
event pumps / frames, the link crates and their replay/collapse/dedup
machinery, contract-core's view-frame vocabulary, replica-projector +
replica-core + client-node-wasm, the web `runtime/` adapters (`httpAdapter`,
`entityStoreAdapter`), `domain-cache`, react-query-as-replica semantics, the
second codegen pipeline, RFC-L2-view-membership-negotiation (whole problem
moot: one evaluator), client-resilience M41–M45/M49–M50 (self-healing a replica
that no longer exists), PLAN-L2-client-link-unification.

**Survives untouched** (the ~90k core, all of it recently hardened): the NS1
overlay/effective substrate and `BaseWrite` seal, NS2 intents/outbox/verdicts,
both provider engines (JMAP/IMAP) and the exactly-once send path, the store and
its migrations, smart mailboxes and the rule compiler (SQL stays the single
predicate engine — permanently, now), undo/redo (per-device), the wizard,
observability, the Stalwart L3 harness. The web `components/` layer (~27k)
survives with its data plumbing swapped underneath.

## 5. Migration slices

- **Slice 0 — the spike (decision gate).** Surface store + patch stream for
  ONE surface (the inbox list), wired into the existing web app behind a third
  `RuntimeAdapter` implementation (`mirrorAdapter`) — the adapter seam the web
  already has (`httpAdapter` vs `entityStoreAdapter`) means the old path keeps
  working untouched. Exit criteria: the L3 convergence test passes against the
  mirror path with a trivial `ViewWatch` (no `MailListMirror` needed — reading
  the document IS the client), the recomputer survives a 20-message burst with
  coalesced (not per-event) recomputes, and the adapter LOC is an order of
  magnitude below the entity-store path. Ratification of this RFC's remainder
  happens on the spike's evidence.

- **Slice 1 — the surface engine.** Sessions + the document model + D183
  recomputer (dirty marking from domain events, tick coalescing, diff/patch
  emission) + D182 recovery + window-extension commands. Reuses the existing
  mail-query/`build_snapshot` code as the materializer. L2 tests: patch/seq
  properties, recovery-equals-connect, recompute-coalescing perf gate.

- **Slice 2 — web cutover, surface by surface.** Mail list → thread/detail →
  sidebar/counts → outbox/verdict pane → search. Each surface moves whole
  (house rule: land green, no half-migrations); the corresponding
  entity-store/adapter code is deleted in the same slice. Compose keeps its
  local buffer; its submit is already one intent (NS2).

- **Slice 3 — retire the seam.** Delete the link/replica crates, far-end view
  machinery, frame vocabulary, WASM store, second codegen; collapse
  http-api-adapter to commands + the session stream + existing REST reads.
  The testkit `MailListMirror` (membership RFC Slice 0) is deleted with it —
  `ViewWatch` consumes the document. One grep proves the flag-era vocabulary
  gone.

- **Slice 4 — one integration surface.** `posthastectl`, MCP, and watch-exec
  move onto the same command API + session stream; scripting docs rewritten
  around "read state, send commands"; the D186 hackability story documented
  with a worked example (a user-added integration touching no build system).

- **Slice 5 — artifact consolidation (severable).** Decide sidecar-vs-embedded
  for desktop; shrink the release matrix; single-versioned update model
  against the one DB. Deliberately last: everything before it is already a
  win if this slice never happens.

## 6. Test contract

- The **L3 convergence test** becomes the canary for the whole model, riding
  the real protocol end-to-end (real Stalwart → sync → overlay → surface →
  patch → client store): the exact test that exposed the old model's
  unverifiable-promise flaw. Graduates `stalwart-integration` to a hard gate
  (the membership RFC's Slice 5, carried over).
- **Recomputer perf gate** at L2: an N-message burst may recompute each dirty
  surface at most once per tick; patches ship diffs, not documents.
- **Recovery-equals-connect** property at L2: for any prefix of patches
  dropped, a document refetch converges to the same state as the unbroken
  stream.
- Provider parity, send exactly-once, and the store suites are untouched — the
  core beneath the surfaces does not change.

## 7. Doc-status changes on ratification

- RFC-L2-view-membership-negotiation → **SUPERSEDED (pre-implementation)**;
  its Slice 0 (`MailListMirror`) stays as the L3 bridge until Slice 3 here.
- RFC-L2-client-resilience → pending tail (M41–M45, M49–M50) closed as
  superseded; landed steps (M40, M46–M48) remain historical record.
- PLAN-L2-client-link-unification → superseded (U1–U5 moot).
- docs/issues/L2-single-source-view-membership → closed (the single source of
  truth is achieved by removing the second consumer, not negotiating with it).

## 8. Links

- RFC-L2-client-replication-model — the substrate (D167–D169) this model reads.
- RFC-L2-send-draft-state-machine — the command vocabulary (intents, verdicts).
- RFC-L2-view-membership-negotiation — the superseded negotiation design; its
  problem statement (§1) doubles as this RFC's evidence file.
- docs/issues/L2-single-source-view-membership — option iii's ledger; this RFC
  is its terminal cleanup.
- RFC-L2-scripting — the automation surface that Slice 4 unifies with the UI.
- https://github.com/earendil-works/pi (packages/coding-agent) — the
  state-mirror + hackable-client shape this model adopts.
