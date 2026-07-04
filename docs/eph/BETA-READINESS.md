# Beta-readiness punch-list (public beta go/no-go)

> Consolidated from parallel audits (2026-07-04). Provider/onboarding audit landed;
> data-safety/sync-correctness audit pending. In-flight fixes tagged [WIP:worker].
> Marks: **BLOCKER** (must-fix before public beta) / SHOULD / POLISH.

## In-flight (this wave)
- **Send/save reliability (M65+M66)** — send/save onto the unified optimistic path (revert-on-failure; no silent send). [WIP:send-save]
- **Recovery & error UX** — human error messages, degraded state, retry/reconnect. Covers B1, B2, S2 below. [WIP:recovery-ux]
- **M67 WS multi-method flush hang** — provider auditor claims already fixed; reproduction found it hangs. [WIP:m67] — VERIFY the discrepancy.

## Provider coverage & onboarding audit

### BLOCKER
- **B1 — Raw lib error strings leak to the user** ("cannot connect to TCP stream"). UI renders `runtime.lastSyncError` raw (AccountEditor.tsx:170) while the classified `lastSyncErrorCode` sits unused (shared.rs:224). Also toasts (useAccountCommandMutation.ts:78). → [WIP:recovery-ux].
- **B2 — No re-auth/retry affordance; errored accounts unrecoverable in place.** authError accounts show only a no-op Sync button (AccountActions.tsx). startProvider OAuth reachable only from add-new, not from an error state. → [WIP:recovery-ux].
- **B3 — No manual IMAP setup and no autodiscovery.** accountForms.ts:75 hardcodes `driver:'jmap'`; manual form collects only JMAP baseUrl. Fastmail/Stalwart-over-IMAP users can't onboard via the web UI despite working IMAP support. NOT YET WORKED.
- **B4 — Crash mid-initial-sync restarts from scratch.** IMAP headers accumulate in memory, persisted by one terminal txn (sync_batch.rs:33); a close at 80k/100k retains zero rows → re-fetch from UID 1. The streamed per-chunk commit path exists for JMAP fallback (sync_batch.rs:355) — extend to IMAP FullSnapshot. NOT YET WORKED. Flaky-network beta users may never finish first sync.

### SHOULD
- **S1 — No usable progress on a large single-folder sync.** `total_count` is a dead field; percent = mailboxIndex/mailboxCount so one giant "All Mail" is frozen 0/100%. Real per-chunk counts exist as log lines only (headers.rs:136). NOT YET WORKED.
- **S2 — Auth failures collapse to one generic message** (no app-password/2FA guidance); connection.rs:367 discards the SASL reason. → partial [WIP:recovery-ux].
- **S3 — OAuth deny/cancel → raw JSON page + frozen settings pane** (no success-HTML for the denied branch; AccountSetupChoice has no polling/timeout/retry). NOT YET WORKED.
- **S4 — Headless/keyring-less Linux → opaque add-account failure** (fails safely, no plaintext fallback — good — but no actionable copy). NOT YET WORKED.

### POLISH
- P1 badge/banner disagree (proves B1 low-effort); P2 JMAP capabilities not negotiated (hardcoded `using`); P3 empty-string id fallback (unwrap_or_default); P4 cross-process token-rotation TOCTOU (accepted residual); P5 macaroon root-key file fallback invisible; P6 dead duplicated secret logic; P7 Gmail TODO(S3) write-guard gap on full-snapshot fallback.

### Confirmed-good (no action)
OAuth scopes minimal; XOAUTH2 correct; Gmail Sent-copy ProviderManaged (no dup); CONDSTORE/QRESYNC capability-gated; OAuth timeouts bounded; secret store OS-keyring, no plaintext fallback, no secret logging; server-side jittered backoff reconnect works.

## Data-safety & sync-correctness audit
Backend overall unusually hardened (outbox exactly-once, drop-guards, atomic prune+cursor). Residual gaps that can LOSE or DUPLICATE mail:

### BLOCKER
- **DS1 — Full-sync prune deletes local mail against an UNPAGINATED Email/query → permanent mail loss.** The JMAP full-snapshot reconcile treats ONE unbounded Email/query id set (engine/sync/email.rs:220-227, no limit/position/loop — contrast the delta path which loops on has_more_changes email.rs:135-168) as complete remote truth, then durably prunes every local message absent from it (store/mutations/sync_batch.rs:310-343, committed atomically with the cursor 417-419). RFC 8620 §5.5 lets servers CAP Email/query (Fastmail/proxies do). Trigger: large account → delta expires (cannotCalculateChanges) → full fallback → query returns only N recent ids → every older local message PRUNED though still on server. Latent (first-sync has nothing to prune → passed on small Stalwart). Aggravator: no floor guard — a transiently-empty-but-Ok query wipes the whole store. FIX: paginate to exhaustion (read total; refuse prune if ids.len()<total) + a floor guard (never prune-by-absence on empty/drastically-smaller remote without an explicit full-resync) + audit the IMAP full-snapshot path for the same. **THE top priority — mail loss.** [WIP:ds1-prune]
- **DS2 — Draft create/update has no deterministic provider id → lost-response retry orphans the committed write → durable TWIN.** draft.rs:37 email_set.create() is anonymous (contrast send.rs:97 create_with_id(phsend-…)); the Transient flush arm re-queues without reconciling the entity id (outbox.rs:690-705). Attempt 2 creates a second draft. FIX: derive the draft create-id from operation.id (like send) or reconcile-by-DRAFT_ID_HEADER. → folded into M65 [WIP:send-save].
- **DS3 — save_draft never checks the destroy(replace) outcome → stale/failed replace silently leaves the old draft** (twin amplifier). draft.rs:96-107 inspects created but never destroyed(replace); the update path swallows EVERY destroy failure (the M60/D133 mask fix only covered the delete path). FIX: inspect destroyed(replace), surface failures. → folded into M65 [WIP:send-save].

### SHOULD-FIX
- **DS4 — Web send/save carry no Idempotency-Key** → keyless resubmit mints a 2nd operation → double send/draft (mutations.ts:204-215; handle.rs:822-828,887-895 bypass the ledger). FIX: stable per-compose-session Idempotency-Key through the ledger. → addressed by M65/M66 routing through runMutation [WIP:send-save].
- **DS5 — Post-commit transport reset on send classified retryable → blind resend → double Sent** (send.rs:155-158 only parks timeouts/truncation). FIX: classify unknown-fate send transport errors as DispatchUncertain.
- **DS6 — Failed submission / post-success Sent-move failure strands a Drafts copy + un-consumes the draft** (send.rs:97-99,174-187). FIX: destroy-on-failure; treat post-success move failure as success-with-warning.
- **DS7 — Apply-ledger in-memory + reap-on-reserve (15min TTL, non-persistent)** → redelivery >TTL or post-restart re-executes (apply_ledger.rs:160,137-139). FIX: persist the keyed decision or dominate the TTL.
- **DS8 — Client fold reverts only via the settlement frame, not a failed receipt** (entityStoreAdapter.ts:742-762; two backstops exist). FIX: revert on receipt state==failed too. → in the M66 bridge's territory [WIP:send-save].

### Confirmed solid (no action)
Sync cursor safety (atomic prune+cursor; mid-stream abort withholds cursor); optimistic convergence kernel (no strand paths); outbox exactly-once for send (deterministic phsend id, DispatchUncertain parking); M35 durable snapshot guard; draft delete/discard D133 mask; apply-ledger dedup LOGIC (only retention/persistence is the gap).
