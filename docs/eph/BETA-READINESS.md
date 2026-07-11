# Beta-readiness punch-list (public beta go/no-go)

> Consolidated from parallel audits (2026-07-04). **RECONCILED 2026-07-10 against HEAD:**
> all four BLOCKERs (B1–B4) and the data-safety BLOCKERs (DS1–DS3) are now FIXED, plus
> most SHOULD-FIX data-safety items (DS4/DS5/DS7). Inline **[✅ FIXED @HEAD]** /
> **[🟠 PARTIAL @HEAD]** tags carry current anchors. Remaining pre-beta items are SHOULD/
> POLISH onboarding polish + a couple of data-safety partials (DS6). Marks: **BLOCKER** /
> SHOULD / POLISH.

## In-flight (this wave) — RECONCILED 2026-07-10
- **[✅ LANDED @HEAD]** Send/save reliability (M65+M66) — unified optimistic path with revert-on-failure (see DS2–DS5, DS8 below, all fixed).
- **[✅ LANDED @HEAD]** Recovery & error UX — classified messages + in-place reconnect (B1/B2 fixed).
- **[⚪ NEEDS-CHECK @HEAD]** M67 WS multi-method flush hang — reconciliation did not re-run the live repro; treat as unverified until a fresh WS-flush hang test is run. (Related BE-H2 outbox head-of-line is still OPEN.)

## Provider coverage & onboarding audit

### BLOCKER
- **[✅ FIXED @HEAD — `apps/web/src/accountHealth.ts:284`, `AccountEditor.tsx:111`]** classified codes now drive user-facing messages (`classifyAccountSetupError`); raw strings no longer rendered. **B1 — Raw lib error strings leak to the user.**
- **[✅ FIXED @HEAD — `apps/web/src/components/settings-panel/AccountHealthNotice.tsx:69`]** errored accounts now show a "Reconnect" action wired from `accountHealth().action`. **B2 — No re-auth/retry affordance.**
- **[✅ FIXED @HEAD — `apps/web/src/components/settings-panel/account-editor/ConnectionEditor.tsx:140`]** a `DriverPicker` now offers jmap / imapSmtp with full IMAP/SMTP endpoint fields. **B3 — No manual IMAP setup.**
- **[✅ FIXED @HEAD — `crates/posthaste-imap/src/gateway/streaming.rs:10-14,165-304`]** IMAP full snapshot now commits per-chunk with an advancing durable cursor; a crash resumes from the cursor, not UID 1. **B4 — Crash mid-initial-sync restarts from scratch.**

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
- **[✅ FIXED @HEAD — `crates/posthaste-engine/src/sync/email.rs:212-221`, `crates/posthaste-store/src/mutations/sync_batch.rs:92-109,461-490`]** full-snapshot pages to exhaustion; `replace_all_messages` gated on `remote_ids_complete`; prune floor-guarded (`MAX_ABSENCE_PRUNE_FRACTION`) + protected-id exemption. **DS1 — Full-sync prune deletes local mail against an UNPAGINATED Email/query → permanent mail loss.** The JMAP full-snapshot reconcile treats ONE unbounded Email/query id set (engine/sync/email.rs:220-227, no limit/position/loop — contrast the delta path which loops on has_more_changes email.rs:135-168) as complete remote truth, then durably prunes every local message absent from it (store/mutations/sync_batch.rs:310-343, committed atomically with the cursor 417-419). RFC 8620 §5.5 lets servers CAP Email/query (Fastmail/proxies do). Trigger: large account → delta expires (cannotCalculateChanges) → full fallback → query returns only N recent ids → every older local message PRUNED though still on server. Latent (first-sync has nothing to prune → passed on small Stalwart). Aggravator: no floor guard — a transiently-empty-but-Ok query wipes the whole store. FIX: paginate to exhaustion (read total; refuse prune if ids.len()<total) + a floor guard (never prune-by-absence on empty/drastically-smaller remote without an explicit full-resync) + audit the IMAP full-snapshot path for the same. **THE top priority — mail loss.** [WIP:ds1-prune]
- **[✅ FIXED @HEAD — `crates/posthaste-engine/src/live_compose/draft.rs:19-25,57-62`]** draft create now uses `create_with_id(draft_create_id(idempotency_key))` → redelivery is server-side idempotent. **DS2 — Draft create/update has no deterministic provider id → lost-response retry orphans the committed write → durable TWIN.** draft.rs:37 email_set.create() is anonymous (contrast send.rs:97 create_with_id(phsend-…)); the Transient flush arm re-queues without reconciling the entity id (outbox.rs:690-705). Attempt 2 creates a second draft. FIX: derive the draft create-id from operation.id (like send) or reconcile-by-DRAFT_ID_HEADER. → folded into M65 [WIP:send-save].
- **[✅ FIXED @HEAD — `crates/posthaste-engine/src/live_compose/draft.rs:137-157`]** the replace path now inspects the destroy outcome (benign `notFound` on redelivery; surfaced error on first delivery). **DS3 — save_draft never checks the destroy(replace) outcome → stale/failed replace silently leaves the old draft** (twin amplifier). draft.rs:96-107 inspects created but never destroyed(replace); the update path swallows EVERY destroy failure (the M60/D133 mask fix only covered the delete path). FIX: inspect destroyed(replace), surface failures. → folded into M65 [WIP:send-save].

### SHOULD-FIX
- **[✅ FIXED @HEAD — `apps/web/src/runtime/mutations.ts` `clientMutationId`, `crates/posthaste-runtime/src/handle.rs:1010-1132` ledger `reserve` w/ distinct op names]** **DS4 — Web send/save carry no Idempotency-Key** → keyless resubmit mints a 2nd operation.
- **[✅ FIXED @HEAD — `crates/posthaste-engine/src/live_compose/send.rs:170-173`]** send dispatch wrapped in `timeout(SEND_TOTAL)` → `dispatch_uncertain` on expiry. **DS5 — Post-commit transport reset on send classified retryable → blind resend → double Sent.**
- **[🟠 PARTIAL @HEAD — `crates/posthaste-engine/src/live_compose/send.rs:197-218`]** post-success Sent-move failure is now logged as success-with-warning (send returns Ok), but the stale Drafts copy is NOT proactively destroyed — it relies on sync reconciliation. **DS6 — Failed submission / post-success Sent-move failure strands a Drafts copy.** *← remaining loose end.*
- **[✅ FIXED @HEAD — `crates/posthaste-runtime/src/apply_ledger.rs:293-315`, `crates/posthaste-store/src/apply_ledger.rs:85-140`]** ledger decisions now persist durably (`pending` marker before execute; re-observed on restart/TTL reap → Conflict, no re-execute). **DS7 — Apply-ledger in-memory + reap-on-reserve.**
- **[✅ FIXED @HEAD — `apps/web/src/runtime/replica/entityStoreAdapter.ts:878-910`]** a receipt resolving `failed`/`conflict` now reverts optimism via `settleAll(..., 'failed')` synchronously, without waiting for a settlement frame. **DS8 — Client fold reverts only via the settlement frame, not a failed receipt.**

### Confirmed solid (no action)
Sync cursor safety (atomic prune+cursor; mid-stream abort withholds cursor); optimistic convergence kernel (no strand paths); outbox exactly-once for send (deterministic phsend id, DispatchUncertain parking); M35 durable snapshot guard; draft delete/discard D133 mask; apply-ledger dedup LOGIC (only retention/persistence is the gap).
