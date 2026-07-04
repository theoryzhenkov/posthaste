---
scope: L2
summary: "RFC (draft) — provider & sync reliability: one outbound-call envelope for every provider call (per-class deadlines, jittered retry + Retry-After, typed failure classification, per-account circuit breaker), send-exactly-once (S1 CRITICAL: idempotent JMAP/SMTP submission + a dispatch-uncertain outbox state), push-lifecycle repair (PP1 keepalive, PP2 reconnect + WS→SSE fallback, PP3 pushState resume + catch-up), IMAP/sync robustness (connection envelope, streaming progress, allocation bounds, CONDSTORE incremental), account-task supervision + scheduling (jitter, global cap, the P5 idle-claim fix, deletion ordering), and OAuth single-flight + CAS rotation. Decisions D80–D102 (reserved D80–D109); rejected R80–R86; migration M30–M37. Companion: RFC-L2-lifecycle-and-errors (failure taxonomy, generic watchdog, process shutdown)."
modified: 2026-07-03
reviewed: 2026-07-03
lifecycle: ephemeral
type: RFC
state: ratified
depends:
  - path: eph/AUDIT-L2-jmap-push
  - path: eph/AUDIT-L2-imap-sync-scheduling
  - path: eph/AUDIT-L2-lifecycle-resources
  - path: eph/RFC-L2-architecture-cleanup
  - path: architecture/L2-crate-topology
dependents: []
---

# RFC — Provider & Sync Reliability (draft)

> **Status (2026-07-04): SHIPPED.** Ratified (owner, 2026-07-03) and fully
> landed: M30–M37 all shipped. Delivered: the `posthaste-call-policy`
> outbound-call envelope (per-class deadlines, jittered retry, circuit breaker),
> send-exactly-once (S1 CRITICAL — idempotent JMAP/SMTP submission +
> DispatchUncertain; M32), push-lifecycle repair (M33), IMAP connection
> lifecycle (M34/M35b), the durable full-snapshot unsettled guard that retired
> the P1/S2 hotfix (M35), supervision/scheduling + the P5 idle-claim fix (M36),
> and OAuth single-flight + CAS rotation (M37). **[Update 2026-07-04]:** the
> "draft — for ratification review" line below and the "(draft)" in the title
> are historical — the RFC was ratified and every M30–M37 row landed.

Status: **draft — for ratification review.** Every row below is `proposed`
(one is `deferred`, awaiting an explicit owner ruling — §6/O1). Nothing here
touches code until a row is ratified and given a migration step + gate (§5).
This is the *reliability* companion to the topology refactor: where
RFC-L2-architecture-cleanup made the provider stack **navigable**, this RFC
makes it **survive the network**.

**Reserved ranges (noted per house convention so parallel RFCs don't collide):**
decision rows **D80–D109** (this RFC uses D80–D102); rejected alternatives
**R80–R89**; migration steps **M30–M39** (this RFC uses M30–M37). The arch RFC
holds D1–D54 and M0–M10; the scripting RFC holds D52–D53. Tenet refs point at
`~/.claude/agent/docs/engineering/L1-principles.md`.

**Companion RFC (referenced, not duplicated): `RFC-L2-lifecycle-and-errors`**
(next-wave sibling; evidence base `AUDIT-L2-lifecycle-resources`). It owns three
generic mechanisms this RFC *consumes at the provider edge*: (a) the canonical
**typed failure taxonomy** (D82 aligns the provider classification to it rather
than minting a second enum); (b) the **generic task watchdog / supervision
mechanism** (D97 specifies the account-runtime *instantiation* of it, not a new
one); (c) **process shutdown/drain** (N1–N3 — out of scope here; the account-task
restart in D97 is the per-account layer beneath it). Where a decision needs a
mechanism that RFC owns, this RFC states the instantiation and cites it.

---

## 1. Evidence summary

Two audits are the evidence base: `AUDIT-L2-jmap-push` (JMAP engine + push
pipeline; findings F/P/PP/S/A/C) and `AUDIT-L2-imap-sync-scheduling` (IMAP
gateway + supervisor/scheduling; findings C/P/K/S/Sc/R/D), cross-checked against
`AUDIT-L2-lifecycle-resources` (N-rows) where the same seam is implicated. Both
audits reach the same verdict independently: the subsystem is **correct at rest,
fragile in motion** — mature layering and single-transaction persistence (S0,
K2), *first-draft* network edge: not one retry, jitter, keepalive, or per-class
deadline exists; one 10-second total timeout governs a keyword flip and a 20 MB
blob alike; the one operation where at-most-once actually matters (send) retries
a non-idempotent call. The provider stack is "roughly one refactor generation
behind" the link stack — which already has the exact primitives it lacks
(jittered backoff, fallback ladder, checkpoint-resume). This RFC's throughline:
**stop hand-rolling those primitives per-engine; instantiate the M9 substrate's
policy discipline at the provider edge.**

| Scope area | Governing findings | Decisions |
|---|---|---|
| 1. Provider-call envelope | F1 (no retry/429), F2 (10s monoculture, **High**), F3 (`Network(String)` collapse), C1/C5-IMAP (no timeout, **High**), N11 (OAuth no timeout) | D80–D83 |
| 2. Send-exactly-once (**CRITICAL**) | **S1** (duplicate send on timeout), P3 (positional response trust feeds S1) | D84–D87 |
| 3. Push lifecycle | PP1 (silent push death, **High**), PP2 (reconnect counter reset + no fallback, **High**), PP3 (no catch-up + `pushState` unused), PP6 (no terminal state) | D88–D91 |
| 4. Sync robustness | **P1-IMAP** (full-snapshot prunes in-flight local msgs — *hotfixed today*), S2/S3-JMAP (snapshot clobber + no checkpoint), K1/R1 (whole-account buffered), R2/R3 (server-driven allocation), P2 (CONDSTORE non-incremental) | D92–D96 |
| 5. Supervision & scheduling | S1-IMAP (no restart/watchdog, **High**), Sc1/R4 (herd, no global cap), **P5** (idle-claim coalescing race + flaky test), D1/D2 (delete ordering + orphan rows) | D97–D100 |
| 6. OAuth | A1 (refresh race → permanent lockout, **High**), A2 (`invalid_grant` swallowed by tick) | D101–D102 |

The two criticals get a dedicated section (§2) before the decision log because
they are user- and third-party-visible and each threads several rows.

---

## 2. The two criticals

### 2.1 S1 — duplicate outbound email on send timeout (CRITICAL)

**The chain, as audited** (`AUDIT-L2-jmap-push` S1, F2, P3):

1. `send_message` inherits the 10 s *total-request* timeout (F2; jmap-client
   `DEFAULT_TIMEOUT_MS`, `J/client.rs:43,97,216`). A server that takes 11 s to
   accept a submission times out **client-side after the server already
   committed it**.
2. The reqwest timeout error → `GatewayError::Network` (F3, `live.rs:199`) →
   `classify_gateway_error` = **`Transient`** (`domain-service/.../outbox.rs:31-34`).
3. Transient → the op is reset to `Pending`, `attempts += 1`
   (`outbox.rs:492-501`) → **auto-resent on the next flush** (`outbox.rs:584-593`,
   a poll tick ≤60 s later).
4. The JMAP send is `Email/set create` + `EmailSubmission/set` with **no
   client-supplied idempotency key** (`live_compose/send.rs:30-83`). The resend
   re-creates and re-submits. The recipient gets the mail twice; it repeats every
   timeout. The existing "send-once" guard only catches ops *stuck Inflight from a
   crashed flush* (`outbox.rs:391-420`) — the errored path routes around it, and a
   code comment scopes the concern to SMTP only.
5. P3 (positional trust of `methodResponses`, `send.rs:84-109`) is a *second
   feeder*: a server that interleaves/reorders responses makes a **succeeded** send
   look failed, driving the same resend.

**The resolution is three moves plus one deferred ruling:**

- **Make submission idempotent** so a resend cannot double-execute — JMAP
  `EmailSubmission` with a client-supplied create-id + `ifInState`
  precondition (**D84**); SMTP dedup by `Message-ID` with honest per-protocol
  limits (**D85**).
- **Make the outbox stop blind-resending** — a `DispatchUncertain` state that a
  timed-out/ambiguous send enters instead of `Transient→Pending`; it is *never*
  auto-resent, only reconciled or (idempotently) re-forwarded (**D86**).
- **Rule the interim behavior the owner deferred** — on `DispatchUncertain`, do
  we **park + surface** (let the user confirm/discard) or **bounded auto-retry
  under the idempotency key**? This is a product-visible call, laid out as an
  explicit decision row with options but **left `deferred` for the owner** (**D87**;
  restated in §6/O1). Until it is ruled, the *safe interim* is park + surface —
  never resend a possibly-delivered message.

The envelope work (D81 per-class send deadline, D82 typed classification) is a
prerequisite: `DispatchUncertain` is exactly the classification *timeout ∨
ambiguous-response* on the send class, which today collapses into `Network` and
is mis-labeled `Transient`.

### 2.2 P1-IMAP full-snapshot prune (hotfixed today) → the durable S3 design

**P1-IMAP** (`AUDIT-L2-imap-sync-scheduling` P1, **High**): a full-snapshot sync
sets `replace_all_messages = true` (`imap/src/sync.rs:94`); the unsettled-write
guard early-returns *exactly then* (`domain-service/.../sync_ops.rs:31-33`,
`if unsettled.is_empty() || batch.replace_all_messages { return; }`, `TODO(S3)`);
the store then runs `prune_messages_absent_from_remote_tx`
(`store/.../sync_batch.rs:44-56`) and **deletes a locally-created message still
in flight**. On a plain IMAP4rev1 server (no CONDSTORE/QRESYNC → *always* a full
snapshot, `planning.rs:50-54`) this is a data-loss window, not an edge case. Its
JMAP twin is **S2** (`replace_all` snapshot clobbers unsettled optimistic writes
on the `cannotCalculateChanges` fallback — same `TODO(S3)`).

**Landed today (parallel hotfix — reference, do not re-litigate):** the narrow
stopgap makes `guard_unsettled` stop early-returning on `replace_all_messages`
and instead exempts rows backing an unsettled local op from the prune, closing
the plain-IMAP data-loss window. It is a **point patch on the IMAP path**; it
does not unify the two `TODO(S3)` sites nor define the reconcile semantics for a
snapshot that legitimately supersedes a pending write.

**The durable design (D93):** one guard, both paths (IMAP full snapshot *and*
JMAP `cannotCalculateChanges` fallback), expressed as **fold-pending-over-
snapshot-before-prune**: a full snapshot is reconciled against the domain-service
outbox's pending set — a row absent from the snapshot is pruned **only if no
unsettled op targets it**; a row present-but-stale is upserted with the pending
op still folded on read. This is the *provider→store* analogue of the client's
`OptimisticReplica` retire invariant (D34/D35a fold discipline) — the same
"pending until the base absorbs it" rule the M6 kernel already owns, applied at
the sync-apply seam rather than re-derived. D93 supersedes the hotfix (the
hotfix's exemption becomes the general fold) and closes both `TODO(S3)` sites at
once. It composes with D94 (streaming progress) but is independent of it.

---

## 3. Decision rows (D80–D102)

Reserved band D80–D109. All rows `proposed` except D87 (`deferred`). Format
matches the arch RFC's decision log.

### §3.1 The provider-call envelope (scope 1)

| Ref | Decision | Rationale (tenet) | Status |
|-----|----------|-------------------|--------|
| D80 | **One outbound-call envelope for every provider call, built on an *extracted policy core*.** Factor the wasm-pure policy of `posthaste-link-near-end` (deadline schedule, jittered exponential backoff, `Retry-After`/429 arithmetic, the classification taxonomy) into a shared **`posthaste-call-policy`** crate (serde-only, joins the D15 frontier); `posthaste-link-near-end` is retrofitted onto it (proving the shared-fact claim, not asserting it). A thin **native** envelope (`posthaste-provider-call`, tokio/reqwest) wraps it with the *executor* half — applying the deadline via `tokio::time::timeout`, running the retry loop, and holding the per-account circuit breaker (D83). Every JMAP call (`posthaste-engine`), IMAP command (`posthaste-imap`), SMTP send, and OAuth IdP call (N11) routes through it. **Where it lives:** policy core = new wasm-pure crate (shared with the link engine); executor = new native crate; neither is bolted onto a single provider engine. Kills F1 (no retry anywhere), gives F3/A2 a home. | XIV (one policy, forked 3×+ today); XV (name the boundary once); II | proposed |
| D81 | **Per-class deadlines replace the 10 s monoculture (F2).** Three classes, three deadline shapes: **metadata/mutation** — a short *total* deadline (~30 s, tunable); **blob** (download/upload) — **no total timeout**; instead an *idle/stall read-deadline* (no-bytes-for-N-seconds), because F2's actual defect is applying a *total* timeout to a *streaming* body — a 20 MB blob on a slow link fails deterministically forever; **send** — its own deadline coupled to the `DispatchUncertain` semantics (D86): expiry classifies as dispatch-uncertain, never as a blind-retryable transient. The deadline table lives in the policy core (D80) and is the single tuning surface. | VI (deadline as data, per class); XIII (a 10 s cap that lies about blobs) | proposed |
| D82 | **Typed failure classification — align to the lifecycle RFC's taxonomy, do not mint a second enum.** Replace F3's `GatewayError::Network(String)` collapse (and A2's `invalid_grant` substring match) with the canonical taxonomy owned by `RFC-L2-lifecycle-and-errors`: `Dns`, `Connect`, `Tls`, `Timeout`, `RateLimited{retry_after}`, `Http4xx{status}`, `Http5xx{status}`, `Auth`, `Malformed`. Classification is *data*, computed once at the edge, and drives three consumers: retry-vs-permanent (D80), circuit-breaker accounting (D83), and account-status surfacing (D91/D102). The provider edge is a *producer* of this taxonomy; the lifecycle RFC owns its definition (parse-once, one vocabulary). | III (parse, don't stringly-classify); XIV (one taxonomy, two producers) | proposed |
| D83 | **Per-account circuit breaker.** After *N* consecutive permanent-or-5xx failures on an account's provider endpoint, open the breaker for a cooldown (short-circuit calls → fast `Unavailable`, surfaced as `Degraded`), then half-open with a single probe. Bounds F1's "sync aborts, poll loop re-hammers at full speed" and dampens the reconnect/herd storms (PP2, Sc1). **Per-account, per-endpoint** — never global (R86): one bad account must not open the breaker for healthy ones. Lives in the native executor (D80); thresholds/cooldowns are open values (§6/O4). | XIX (bound the blast radius); VII | proposed |

### §3.2 Send-exactly-once (scope 2 — CRITICAL)

| Ref | Decision | Rationale (tenet) | Status |
|-----|----------|-------------------|--------|
| D84 | **JMAP send idempotency: client-supplied create-id + `ifInState`.** `EmailSubmission/set create` already takes a client-chosen creation id; make it a **deterministic** id derived from the outbox op's `ClientMutationId` (not a fresh UUID per attempt), and carry the submission-state `ifInState` precondition so a resend against an advanced state is rejected rather than duplicated. A server that accepted attempt 1 but lost the response rejects attempt 2's create (id already present / state moved) — the client learns "already submitted," not "submit again." Also hardens P3: correlate responses by create-id, not by `responses.remove(0)` positional index. | X (idempotency key is the contract); III | proposed |
| D85 | **SMTP send idempotency: `Message-ID` dedup — honest per-protocol limits.** SMTP has **no** client-supplied submission idempotency token; the achievable mechanism is a stable, client-generated `Message-ID` per outbox op (constant across retries) so an MTA that honors duplicate suppression drops the second copy. **Stated honestly:** most MTAs do **not** dedup on `Message-ID`, so SMTP cannot be made exactly-once at the protocol level. The SMTP contract is therefore *at-most-once-on-uncertainty*: a timed-out/ambiguous SMTP send goes to `DispatchUncertain` (D86) and is **never auto-resent** — parked for the user (D87/O1). D84's strong guarantee is JMAP-only; D85 is best-effort + a safe default. | X; XIII (don't claim a guarantee the protocol can't keep) | proposed |
| D86 | **Outbox state machine: a `DispatchUncertain` state; never blind-resend a send.** Add `DispatchUncertain` to the outbox op lifecycle, entered when a *send-class* call classifies as `Timeout` or ambiguous-response (D81/D82) — i.e. the exact edge S1 mis-labels `Transient`. A `DispatchUncertain` op is removed from the auto-flush set (`list_flushable`, `store/.../outbox.rs:172-212`): it is resolved only by (a) reconciliation — query submission state by the deterministic id (D84) and settle if present, or (b) an *explicit, idempotent* re-forward under D87's ruling. This generalizes the existing crashed-Inflight guard (`outbox.rs:391-420`) from "crash" to "uncertainty," and applies uniformly to JMAP and SMTP. | X; XXI (name the uncertain case before handling it); VIII | proposed |
| D87 | **[DEFERRED — owner ruling owed] Interim behavior on `DispatchUncertain`.** Two options, both safe given D84/D86; the choice is product-visible: **(A) Park + surface** — the op waits in `DispatchUncertain`; the UI shows "may have sent — confirm or discard"; no automatic action. Safest, zero duplicate risk, adds a user step on every slow-server send. **(B) Bounded auto-retry under the idempotency key** — reconcile-then-re-forward automatically (≤k attempts) relying on D84's create-id to guarantee no duplicate on JMAP; for SMTP (no protocol dedup, D85) option B degrades to option A regardless. Recommendation pending the owner: **A as the interim default** (never risk a third-party-visible duplicate before the ruling), B as the JMAP-only target once D84 is proven in the field. Restated as §6/O1. | X; XIII; product call | **deferred** |

### §3.3 Push lifecycle (scope 3)

| Ref | Decision | Rationale (tenet) | Status |
|-----|----------|-------------------|--------|
| D88 | **Push keepalive / read-deadline + dead-connection teardown (PP1).** Enforce liveness on *both* transports: WS — drive the fork's already-present `ws_ping` (`J/client_ws.rs:354-361`, currently **zero callers**) on an interval with a pong read-deadline; SSE — enforce a *client-side* read-deadline on the server ping it already requests (`push_sse.rs:70`) rather than trusting it. On deadline miss, the connection is declared dead → `disconnect()` fires (today only reached from the push-stream error path, `push_ws.rs:85,95`, which never fires on a NAT half-open), so interactive mutations **stop routing to the dead WS** (`live.rs:134-139`) instead of eating a 10 s timeout each. Read-deadline lives in the shared policy core (D80). | VI (liveness is a deadline); XIII (status must not lie) | proposed |
| D89 | **Reconnect fix: jittered backoff, a counter that survives flaps, and a working WS→SSE fallback (PP2).** Three coupled defects, one fix on the shared near-end policy: (a) **do not reset `consecutive_failures`/delay on every `open()`** — reset only on a *stably-held* connection (held past a min-uptime), so an accept-then-drop server escalates instead of pinning at 5 s forever; (b) **jitter** the exponential schedule (kills the cross-account reconnect herd, PP2/Sc1 — the same jitter D98 adds at startup); (c) fallback then actually triggers (the 3-consecutive-failures threshold becomes reachable → WS→SSE fallback works). This is precisely `LinkNearEnd`'s resilience policy (D40), which already got this right — the provider push loop instantiates it rather than re-hand-rolling `push.rs:44-45,153-154`. | XIV (one reconnect policy, not a broken copy); XIX | proposed |
| D90 | **`pushState` resume + catch-up sync on reconnect (PP3).** On `PushStreamEvent::Connected`, (a) trigger a catch-up incremental sync (today it only updates status → guaranteed notification-loss window up to the 60 s poll); (b) capture the WS reconnect checkpoint — today only SSE populates it (`push_common.rs:36`), WS yields `checkpoint: None` so JMAP `pushState` resume is entirely unused on the *preferred* transport. Carry `pushState` through the WS path and resume `enable_push(pushState)` from it. Whether checkpoint-resume beats an unconditional delta-on-reconnect is an open trade (§6/O6); the catch-up-sync half is unconditional. | VIII (no silent loss window); X | proposed |
| D91 | **Push-death detection surfaces to account status (PP1/PP6).** `PushStatus::Connected` is set once and never revalidated (`runtime.rs:353-363`); the D88 read-deadline makes death *detectable*, and this row makes it *visible*: a dead/again-reconnecting stream flips push status; a structurally-broken push (SSE-only server with a bad `eventSourceUrl`, PP6) reaches a **terminal `Unsupported`/`Failed`** state instead of cycling `Reconnecting` forever. Push status is an input to the D82→account-status mapping, not a lie the 60 s poll papers over. | XIII (status tells the truth); VIII | proposed |

### §3.4 Sync robustness (scope 4)

| Ref | Decision | Rationale (tenet) | Status |
|-----|----------|-------------------|--------|
| D92 | **IMAP connection envelope (C1/C2/C5).** (a) **Timeouts everywhere** — connect/command/read on the IMAP session (`discovery.rs:121-131` has none, C1 — a post-TLS-silent server wedges the *whole account runtime*) and a `.timeout()` on the SMTP transport (`smtp/transport.rs:87-92`, C5); all via the D80 executor. (b) **IDLE re-issue** — a ~29-min `DONE`/re-`IDLE` cycle and a max-IDLE duration (`idle.rs:38-40` is a single un-timed `idle().await`, C2 — a silently-dropped idle socket kills push until the OS tears it down), with jittered backoff on IDLE-reject (C3) instead of the flat 30 s. (c) **Pool-or-not: a single reused authenticated session per account** (not fresh-connect-per-operation, C4/R83; not a general pool — premature, §6/O3), with short-lived side connections capped by D98's global limiter. | VI; XIX; C1 is the audit's #1 IMAP finding | proposed |
| D93 | **Full-snapshot unsettled-write guard — the durable S3 (both paths).** One guard replacing the two `TODO(S3)` early-returns (IMAP P1 `sync_ops.rs:31-33`; JMAP S2 same site): a full snapshot reconciles against the domain-service pending set — **fold-pending-over-snapshot-before-prune** (§2.2). Prune a row absent from the snapshot **only if no unsettled op targets it**; upsert present-but-stale rows with the op still folded. Supersedes today's landed IMAP-only hotfix (its exemption becomes the general fold) and closes the plain-IMAP data-loss window *and* the JMAP `cannotCalculateChanges` UI-revert. Reuses the M6 `OptimisticReplica` retire discipline (D34) at the sync-apply seam. | III; XIV (one guard, both providers); X | proposed |
| D94 | **Streaming / partial progress for large syncs (K1/R1/S3).** Extend the store's already-proven **withheld-cursor streaming** (S0/K2 — chunks commit data + withhold the cursor until reconciliation, so a committed cursor never runs ahead of data and retries re-upsert idempotently) to the two places that lack it: **IMAP** emits the whole account as one batch (`gateway/execution.rs:52-58`, K1/R1 — OOM + one giant write-lock on a 100k mailbox) → per-mailbox chunking; **JMAP full snapshot** has no per-page high-water mark (S3 — an 80k mailbox on a flaky link refetches O(N) from scratch every attempt, never reaching reconciliation) → a per-page checkpoint with withheld-cursor semantics. Delta accumulation (S4) is bounded by the same chunking. | XIV (reuse the store's proven streaming); XIX; K1/R1 = audit #5 | proposed |
| D95 | **Bounds on server-driven allocation (R2/R3).** Cap the VANISHED-range expansion (`fetch/changed_since.rs:169-176` iterates a server-declared range to `NonZeroU32::MAX` — `VANISHED 1:4294967295` → ~4 B-iteration DoS) and ceiling the raw-literal allocation (`body.rs:126` trusts the server-declared size). Both are hostile-input allocation surfaces; the fix is a hard cap + typed rejection, not a bigger buffer. | IV (never trust server-declared size); XIX | proposed |
| D96 | **CONDSTORE incremental execution (P2).** The planner emits `CondstoreDelta{since_modseq, after_uid}` (`planning.rs:42-47`) but the executor destructures `{ .. }` and full-fetches every header (`execution.rs:197,220-232`); only QRESYNC uses the real `CHANGEDSINCE` path. Wire `since_modseq`/`after_uid` through the executor so CONDSTORE-only servers (Dovecot / Fastmail-without-QRESYNC) fetch incrementally instead of re-pulling all headers every cycle. Pure bandwidth/liveness; no correctness change. | XXI (execute the delta you already planned); efficiency | proposed |

### §3.5 Supervision & scheduling (scope 5)

| Ref | Decision | Rationale (tenet) | Status |
|-----|----------|-------------------|--------|
| D97 | **Account-task supervision: restart with backoff + panic capture; watchdog is the lifecycle RFC's mechanism, instantiated here.** Today nobody watches the `JoinHandle` (`manager.rs:72-98`); a panic at any `.await` **silently kills an account** with no status change and no restart until an external `start_account` (S1-IMAP, **High**; N2). This row: (a) wrap the account runtime so a panic/exit is *captured* (join the handle / `catch_unwind` seam) and the account flips to a truthful status; (b) **restart with jittered backoff** (bounded), not a bare respawn; (c) **watchdog** — do **not** invent one: instantiate the generic task-watchdog `RFC-L2-lifecycle-and-errors` owns (heartbeat + liveness) for the account-runtime, so a *wedged-but-not-panicked* loop (row 5 / N17 — a hung provider inline-awaited in the `select!`) is detected. The D81 per-class deadlines are the first line (a hung call now returns); the watchdog is the backstop. | I; XII (supervise the task you spawn); XIV (reuse the generic watchdog) | proposed |
| D98 | **Startup jitter + a global concurrency cap (Sc1/R4).** (a) **Splay** account start and the initial/periodic sync phase (today all accounts start in a tight loop each firing an immediate Startup sync, poll phases near-aligned, zero jitter → an N-account boot storm and a recurring 60 s aligned herd). (b) A **global concurrent-sync limiter** across accounts — the one existing global limiter (`CacheResourceGovernor`) throttles cache fetches only; the sync path never consults it (R4), so 50 accounts syncing at boot = 50 unthrottled provider connections. Whether to extend the cache governor or add a sync governor is open (§6/O7). | XIX (bound fan-out); Sc1/R4 | proposed |
| D99 | **P5 coalescing idle-claim fix + de-flake the test.** The audit's verdict names the fix: the coalescer collapses a trigger only while `syncing==true` (`types.rs:105-113`), but nothing sets `syncing` until the runtime dequeues the first trigger and calls `begin_cycle` — so N concurrent triggers observing the idle window each enqueue a distinct cycle (a **real race**, benign symptom: redundant syncs). Fix: **an atomic claim at the idle boundary** — the first trigger to observe idle atomically claims the cycle (CAS the idle→syncing transition), so the rest coalesce. De-flake `rapid_mutation_burst_coalesces_provider_sync_triggers` (`tests/authority_server_handle.rs:2393-2500`): the `additional_cycles <= 2` assertion encodes an implicit "at most one idle-race winner" that is probabilistic today; with the atomic claim it becomes **invariant** — the test asserts the invariant, not a scheduling accident. | XXI; XXIV (make the test assert an invariant) | proposed |
| D100 | **Deletion ordering + store-row GC (D1/D2).** (a) **Stop the runtime before deleting its secret/config** — today `account_repository.delete()` removes config + secret *then* aborts the task (`mutations/accounts.rs:151-160`), so an in-flight sync runs on with its secret gone (OAuth refresh fails) and can still commit **new rows for the just-deleted account** (D1). Reorder: stop-and-drain the runtime first, then delete. (b) **GC the mail store rows** — deletion purges only config + managed secret; messages/mailboxes/events are orphaned in SQLite indefinitely (D2). Decision: account-delete GC's the account's store rows (or tombstones + a reaper); combined with (a) this closes the "aborted sync re-adds rows to a deleted account" hole. | XII (order the teardown); resource release | proposed |

### §3.6 OAuth (scope 6)

| Ref | Decision | Rationale (tenet) | Status |
|-----|----------|-------------------|--------|
| D101 | **OAuth refresh single-flight + compare-and-swap rotation (A1).** Today `OAuthTokenService` is stateless/rebuilt per call with **no lock** (`oauth/service.rs:4-26`) and `refresh_oauth_access_token` blind-writes `secret_store.update` with no CAS (`connection.rs:279-295`); on a rotating-refresh-token provider (Google-style) two racing refreshes both POST, the loser's `update()` lands last and persists the **consumed** refresh token → every future refresh returns `invalid_grant` → **permanent lockout** (A1, **High**, audit #2). Fix: (a) **single-flight** — one in-flight refresh per account, concurrent callers await its result (a per-account async lock/`OnceCell`, not a per-call rebuild); (b) **compare-and-swap** the secret update — write only if the stored refresh token still matches the one this refresh consumed (`SystemSecretStore::update` is a plain overwrite today, `secret.rs:48-50`), so a stale generation's write cannot clobber the current token. | X (rotation is a CAS, not last-writer-wins); XIV (one refresh in flight) | proposed |
| D102 | **`invalid_grant` → `AuthError` propagation from the refresh tick (A2).** The periodic tick swallows every error as `ph_warn!` + return (`runtime.rs:412-423`) — a revoked grant logs a warning forever and the account only flips `AuthError` when a connection-rebuild happens to run; status lies in the interim. Fix: a `Auth`-classified error (D82, replacing the `invalid_grant` substring match, `support.rs:41-51`) raised from the tick flips the account to `AuthError` immediately. Pairs with D91 (the same truthful-status principle for push). | XIII (status must not lie); III | proposed |

---

## 4. Rejected alternatives (R80–R86)

| Ref | Rejected | Rationale |
|-----|----------|-----------|
| R80 | **Literally instantiate `LinkNearEnd`'s full engine for provider calls** (the prompt's explicit alternative to D80's policy-core extraction). | `LinkNearEnd` is a *subscribing link* engine: forward-POST + frame subscription + cursor/reconciler + settlement (D40). A provider call is request/response — it has no subscription, no cursor, no settlement. Instantiating the whole engine drags machinery a provider call never uses and forces provider semantics into a link shape. What is genuinely shared is the **policy** (deadline/backoff/classification), not the engine — so extract the policy core (D80) and let both the link engine and the provider executor instantiate it. (XIV: share the fact, not the frame.) |
| R81 | **SMTP exactly-once via a full transactional-id / LMTP-style submission ledger.** | SMTP has no client-supplied submission idempotency token; a local ledger cannot make a remote MTA idempotent. The honest ceiling is `Message-ID` dedup *if the MTA honors it* (most don't) — D85 states that limit and falls back to at-most-once-on-uncertainty (D86/park) rather than pretending to a guarantee the protocol can't keep. (XIII.) |
| R82 | **Keep auto-retry-on-timeout as the default send behavior** (status quo). | This *is* the S1 bug: a timeout on a possibly-delivered send, retried without idempotency, duplicates the mail to the recipient. Rejected outright; D86 removes send from the blind-flush set. |
| R83 | **A general IMAP connection pool** (or keep fresh-connect-per-operation, C4). | Fresh-connect-per-op causes connection storms / rate-limit trips (C4). A general pool is premature complexity for a per-account mail client. D92 chooses a **single reused authenticated session per account** + capped side connections — enough to kill the storm without a pool's lifecycle burden. (XX: no speculative pool; §6/O3 revisits if evidence demands.) |
| R84 | **Task-per-operation to solve head-of-line blocking** (the single `select!` loop, C1-jmap/S2-imap/N17). | Decoupling the per-account `select!` loop is real but is the **lifecycle RFC's** concern (row 5 / N17 — inline-awaited ticks). This RFC's D81 per-class deadlines *bound* the head-of-line blocking (a hung call now returns) as the reliability-side mitigation; the structural decoupling stays with the loop's owner to avoid a second, drifting design of the same loop. (XV: one owner per seam.) |
| R85 | **JMAP send idempotency via "search the Sent folder before resending."** | Racy (delivery lag between submit and Sent-append), provider-specific, and defeated by servers that don't reflect submission into Sent promptly. D84's `EmailSubmission` create-id + `ifInState` is the protocol-native mechanism — the server enforces uniqueness, the client doesn't guess. (III/X.) |
| R86 | **A global (cross-account) circuit breaker.** | Provider faults are per-account/per-endpoint (one account's expired cert, another's rate-limit). A global breaker lets one bad account fast-fail all healthy ones. D83 keys the breaker per-account-per-endpoint. (XIX: bound the blast radius, don't widen it.) |

---

## 5. Migration sequence (M30–M37) with gates

Reserved band M30–M39. Each step names its decisions, its dependency, and a
**gate** (the regression that must pass before the step is done). The P1-IMAP
hotfix is already landed; M35 supersedes it with the durable guard.

| Step | Content | Deps | Gate |
|------|---------|------|------|
| **M30** | **Policy-core extraction.** Extract `posthaste-call-policy` (wasm-pure: taxonomy D82, jittered-backoff schedule, `Retry-After`/429 arithmetic, per-class deadline table D81); retrofit `posthaste-link-near-end` onto it. | D80, D81, D82 | `link-near-end` tests green on the extracted policy (proves shared-fact, not a fork); wasm frontier CI still builds the (now six→seven) frontier crate list. |
| **M31** | **Native provider-call envelope.** Build `posthaste-provider-call` (executor: timeout wrap, retry loop, per-account circuit breaker D83) over the policy core; route the JMAP engine's outbound calls through it (F1/F2/F3). | M30, D83 | A >10 MB blob download on a throttled link **completes** (F2 regression — total-timeout→stall-deadline); a 429 with `Retry-After` is honored, not re-hammered (F1). |
| **M32** | **Send-exactly-once.** JMAP `EmailSubmission` deterministic create-id + `ifInState` (D84); SMTP stable `Message-ID` (D85); outbox `DispatchUncertain` state, removed from the flush set (D86). **Interim behavior gated on the D87 owner ruling** — ship option A (park + surface) as the safe default until ruled. | M31, D84, D85, D86, **D87** | The S1 regression: a send that times out after server-commit, then flushes, produces **exactly one** submission (idempotency) and the op sits `DispatchUncertain`, not resent (outbox). |
| **M33** | **Push lifecycle.** Keepalive/read-deadline + dead-WS teardown (D88); reconnect counter/jitter/fallback on the shared policy (D89); `pushState` resume + catch-up sync (D90); push-death → status (D91). | M30, D88–D91 | Accept-then-drop server **escalates** backoff and **falls back** WS→SSE within the threshold (PP2 regression); a NAT-half-open stream flips to `Reconnecting` within the read-deadline and mutations stop routing to the dead WS (PP1). |
| **M34** | **IMAP connection envelope.** Connect/command/read + SMTP timeouts (C1/C5); ~29-min IDLE re-issue + max duration + jittered IDLE-reject backoff (C2/C3); single reused session per account (C4). | M31, D92 | A server that completes TLS then never answers **does not wedge** the account runtime (C1 regression, the audit's #1). |
| **M35b** *(fork audit finding, 2026-07-03 — the THIRD Gmail cause, likely the "silent/intermittent" one)* | **gmail_label parser panics on non-ASCII labels.** The imap-codec fork's `gmail_label` (imap-codec/src/fetch.rs:289-298) has two `.unwrap()`s over server bytes: any non-ASCII Gmail user label (accented/CJK/emoji, or a literal) PANICS the FETCH parse; a quoted-UTF-8 label silently fails the whole response. Data-dependent, account-specific, silent → "unreliable for some reason". Root: `Text` is 7-bit by definition, wrong carrier for UTF-8 labels. Fix (audit-prescribed): change `MessageDataItem::GmailLabels(Vec<Text>)` → a UTF-8-capable carrier (Vec<Cow<str>>/String via from_utf8_lossy — never panic, never drop), fix the encoder + the consumer (posthaste-imap/src/fetch/items.rs:87), add a fuzz/unit target over the label parser (the vendor commit DELETED the fuzz harness — restore at least a targeted test). Push the fork, bump the Cargo.toml rev (line 130). **Verdict: KEEP the fork — async-imap/imap don't support X-GM at all; the CONDSTORE/QRESYNC/VANISHED/MSGID/THRID patches are correct and worth keeping; only gmail_label is defective.** Owned by the Fable IMAP unit. | D96-adjacent | — | A FETCH with a non-ASCII X-GM-LABEL parses (no panic, label preserved); fuzz target over the label parser. |
| **M35a** *(pulled forward — the Gmail full-sync bug, owner-reported 2026-07-03)* | **The executor honors the incremental plan.** CONFIRMED: planning.rs correctly emits `CondstoreDelta { since_modseq, after_uid }` but execution.rs:197 destructures `{ .. }` and calls the FULL `fetch_mailbox_header_snapshot` — so every CONDSTORE-without-QRESYNC sync re-fetches ALL headers (Gmail All Mail = tens of thousands, per cycle → bandwidth throttle + slowness). Fix: the CondstoreDelta arm calls the changed-since fetch with since_modseq (mirror the QRESYNC arm at execution.rs:172). D96. Next Fable unit after M34 (runs on M34's connection manager). | D96 | — | A second sync with no server changes fetches ZERO headers (connect-counting/fetch-counting mock). |
| **M35** | **Sync robustness.** Durable full-snapshot unsettled guard, both paths — **supersedes the landed P1 hotfix** (D93); streaming/partial progress for IMAP + JMAP checkpoint (D94); VANISHED/literal caps (D95); CONDSTORE incremental (D96). | M31, D93–D96 | A full snapshot with a pending local op **preserves** the op (P1-IMAP *and* S2-JMAP regression, replacing the hotfix's narrow exemption); `VANISHED 1:MAX` is capped, not iterated (D95). |
| **M36** | **Supervision & scheduling.** Account-task restart + panic capture + watchdog instantiation (D97); startup jitter + global sync cap (D98); atomic idle-claim + de-flake (D99); deletion ordering + row GC (D100). | M31 (deadlines), D97–D100 | An injected panic in an account runtime → **restart with backoff + truthful status** (S1-IMAP regression); `rapid_mutation_burst...` is now **deterministic** (D99); delete-during-sync commits **no** rows for the deleted account (D100). |
| **M37** | **OAuth hardening.** Refresh single-flight + CAS rotation (D101); `invalid_grant` → `AuthError` from the tick (D102). | M30 (taxonomy for A2), D101, D102 | Two concurrent refreshes → **one** network call and **no** last-writer-wins token loss (A1 regression, the audit's #2); a revoked grant flips `AuthError` from the tick, not on next rebuild (A2). |

Ordering notes: M30 gates everything (the shared policy is the keystone). M33
(push) and M34 (IMAP) both sit on M30's policy + M31's executor and are
otherwise independent. M32 (send) is the highest-priority *behavioral* fix but
is gated on the D87 owner ruling for its interim behavior — it can ship option A
without the ruling. M37 (OAuth) is largely independent and can run early if the
A1 lockout is field-observed.

---

## 6. Open questions

- **O1 — [THE DEFERRED S1 RULING] `DispatchUncertain` interim behavior (D87).**
  Park + surface (A) vs bounded auto-retry under the idempotency key (B). A is
  the safe interim (never risk a duplicate before the ruling); B is the
  JMAP-only target once D84's create-id is field-proven; SMTP degrades B→A
  regardless (D85). **Owner ruling owed** — this is the decision the owner
  explicitly deferred. It gates M32's shipped behavior (not M32's mechanism).
- **O2 — Home of the failure taxonomy (D82).** Does the canonical enum live in
  the lifecycle RFC's crate, in `domain-model`, or in the extracted
  `posthaste-call-policy`? This RFC *consumes* it and must not fork it —
  coordinate with `RFC-L2-lifecycle-and-errors` before M30 so the taxonomy has
  exactly one owner.
- **O3 — IMAP connection strategy (D92).** Single reused authenticated session
  per account (proposed) vs a small bounded pool vs keep-per-op-with-timeouts.
  Recommendation: single session; revisit only on pooling evidence (mutation
  burst latency). Which one is the M34 default?
- **O4 — Circuit-breaker thresholds/cooldowns (D83).** Consecutive-failure count
  to open, cooldown duration, half-open probe cadence — need concrete defaults;
  proposed starting point ~5 failures / 30–60 s cooldown, owner to confirm.
- **O5 — SMTP dispatch-uncertain contract (D85/D86).** For MTAs that don't dedup
  on `Message-ID`, is at-most-once-on-uncertainty (park, never resend) the
  accepted SMTP send contract? This is the SMTP-specific face of O1 and may be
  ruled together with it.
- **O6 — `pushState` resume vs unconditional delta on reconnect (D90).** Is
  capturing the WS reconnect checkpoint (unused today) worth the wire/state cost
  over always running a normal incremental delta on `Connected`? The catch-up
  sync is unconditional either way; only the resume *mechanism* is in question.
- **O7 — Global sync concurrency limiter (D98).** Extend the existing
  `CacheResourceGovernor` (which today throttles cache fetches only) to cover the
  sync path, or add a distinct sync governor? And what is the global cap value?

## 7. Rulings (owner, 2026-07-03)

- **O1 (the S1 ruling): Option A — park + surface.** `DispatchUncertain` sends are parked, never auto-resent; JMAP may graduate to bounded auto-retry (B) only after D84's idempotent create-id is field-proven. **Owner rider: this requires a proper frontend surface** — parked sends become a first-class needs-attention state in the UI (outbox pane: the parked message with its uncertainty reason + explicit retry/discard actions + a notification on entry). Added to M32's scope as its user-facing half; M32 is not done until the surface ships (a parked send with no UI is data loss with extra steps).
- **O2**: the failure taxonomy lives in `posthaste-domain-model` (ruled jointly with the lifecycle RFC §7.3).
- **O3**: single reused authenticated IMAP session per account; revisit only on latency evidence.
- **O4**: circuit breaker defaults accepted — 5 consecutive failures / 30-60s cooldown / single half-open probe; tagged for tuning under real traffic.
- **O5**: at-most-once-on-uncertainty is the accepted SMTP send contract (park, never resend) — the SMTP face of O1's ruling.
- **O6**: skip the WS pushState checkpoint; always run the incremental delta on reconnect (catch-up is unconditional; the resume mechanism can come later on evidence).
- **O7**: a distinct sync governor, not an extension of CacheResourceGovernor — different pressure signals, don't couple unrelated budgets.

## M34 adversarial review (2026-07-03, opus) — CLEAN pass
No data-loss / hang / desynced-session / deadlock / silent-sync-failure found. Verified solid: dirty-flag invariant (set/cleared with no intervening await → cancellation-safe), lease Drop-safety (timeout/panic never wedge the slot mutex), IDLE-recall permit semantics (no missed-wake to the 24min ceiling), FIFO-fair op-before-IDLE re-hold (no deadlock/starvation), no socket/task leak, OAuth rotation can't misclassify, single-flight lock RAII-released (slot→flight order, no cross-account deadlock), codec Cow fix correct (labels read-only, lossy decode → graceful mailbox-mapping miss not panic). Deferred (bounded-latency only, NOT bugs): (#1) reconnect holds the slot mutex across resolve_secret+connect+auth — a latency stall bounded by the 300s arm-budget; fix = connect-detached-then-swap, gated on O3 latency evidence. (#2) idle_wait can fire one premature connect past backoff — self-limiting.

## Field bug (2026-07-04, Stalwart) — cache_maintenance arm wedges the account until reload
Two adversarial hunts. The supervisor 'cache_maintenance' arm exceeds ARM_BUDGET_CACHE (120s) and the account is stuck until reload — AND it starves the outbox flush (so draft-discard etc. don't run while wedged). Root cause is STRUCTURAL (provider-agnostic — hit on JMAP): (1) the body-cache batch (body_worker.rs, up to fetch_request_burst=8 full bodies/lease) is NOT bounded to the arm budget — one slow batch legitimately exceeds 120s; (2) the arm-budget tokio::timeout DROPS the batch future, so resource_governor.rs record_feedback (which sets backoff_until) NEVER runs → backoff never engages → the 2s CACHE_WORKER_INTERVAL re-fires immediately → PERPETUAL RECURRENCE = 'stuck until reload'; also the in-flight candidate is left Fetching (never Failed) so it leaks; (3) the single supervisor select! loop means the 120s cache arm starves command_rx (outbox/discard flush) + poll_sync. NOT a session/connection deadlock (arm-drop releases the lease cleanly). FIX (Fable): bound process_body_cache_batch under a deadline << arm budget (cap request_limit×per-op or a batch deadline); engage governor backoff on CANCEL (drop-guard / mark on record_arm_timeout) to break recurrence; mark the in-flight candidate Failed on cancel; add a byte-progress stall guard to the body fetch (parity with the M34 provider-call Blob class). Files: supervisor/{cache,runtime,types}.rs, domain-service/service/cache/body_worker.rs, cache/resource_governor.rs, imap/body.rs (+ the JMAP body path).
