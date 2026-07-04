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
_(pending — worker a054e3cf running)_
