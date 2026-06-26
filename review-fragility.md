# Posthaste — Fragility / Correctness / Security Audit

> Generated 2026-06-26. Scope: highest-risk code in the most active crates
> (`domain`, `api`, `engine`, `runtime`, `authority-runtime`, `store`, `imap`,
> `server`, `config`), per `review-context.md`. Findings are ordered by severity.
> No files were edited.

## Summary of the trust model (calibrates severity)

The API is a per-process loopback daemon. AuthN is a per-install macaroon
(`Authorization: Bearer`), with a mandatory `Host` allowlist (DNS-rebinding
defense), an `Origin`/`Referer` allowlist (CSRF), and CORS restricted to an
explicit origin list (not `*`) with **no** `allow_credentials` — so the
`allow_methods(Any)`/`allow_headers(Any)` in `serve.rs` is acceptable (the bearer
header, not a cookie, is the credential). Email HTML is rendered in an iframe
with **no `allow-scripts`** (`apps/web/src/components/EmailFrame.tsx:126`), which
is the load-bearing XSS boundary. Several findings below are downgraded because
of these two facts; they are still real defense-in-depth gaps.

The two strongest crosscutting issues are (1) **unbounded per-session mutation
state** and (2) **poisoned-mutex permanent bricking** of central subsystems.

---

## Critical

_None found that survive the loopback + no-scripts-iframe trust model. The auth
perimeter (`auth/perimeter.rs`, `auth/middleware.rs`), JWT/OIDC validation
(`oauth/openid.rs`, RS256-pinned, full manual claim checks, single-use OAuth
`state` via `flow_store.rs`), and the smart-mailbox SQL compiler
(`smart_mailboxes/*` — fixed column literals, fully parameterized values) are all
sound._

---

## Important

### I1. Per-session mutation state grows without bound (memory leak + unbounded reconnect cost)
**File:** `crates/posthaste-runtime/src/sessions.rs:44-46`, `accept_mutation`
(~`:360-372`), `settle_mutation` (~`:502`), `collapse_session_frames`
(`:666`).

`StoredSession.latest_mutations` and `mutations_by_client_id` are inserted into
on every accepted mutation and **never pruned** for the life of the session.
`settle_mutation` mutates entries in place but never removes them. A long-lived
session (the normal case — a desktop client keeps one session open for hours)
accumulates one `StoredMutation` (name + args + output `Value`) per user action
forever.

This compounds with reconnect behavior: `subscribe_frames` →
`collapse_session_frames` (`:660-673`) iterates **all** `latest_mutations` and
re-emits each as a frame with freshly-assigned seqs on every catch-up/lag-collapse.
So reconnect cost (and lag-recovery cost) is O(all mutations ever made in the
session), and the client cannot dedup because seqs are regenerated.

**Fix:** Evict settled mutations after they are acknowledged (e.g. cap like
`undo_history`'s `MAX_HISTORY`, or drop entries once `client_mutation_id` is no
longer needed for idempotency). Only the recent window is needed for replay; the
idempotency index can be bounded with an LRU.

### I2. Poisoned mutexes permanently brick central subsystems
**Files:** `crates/posthaste-store/src/store.rs:152-176` (`read_connection`),
`:179-195` (`write_transaction`); `crates/posthaste-runtime/src/sessions.rs:~750`
(`lock_error`) applied across every `SessionRegistry` method.

Every `std::sync::Mutex` lock maps a poison error to a returned `StoreError` /
`RuntimeError` rather than recovering. Because a `Mutex` stays poisoned after the
first panic-while-locked, a **single** panic inside a write closure (or inside any
code holding the `sessions` lock — e.g. a `serde_json::to_value` that panics, a
`Drop`, a callback) permanently disables that subsystem for the rest of the
process lifetime: all subsequent writes return "write lock poisoned", or every
session operation returns Internal.

The store write path is the single in-process write connection behind one Mutex,
so this is the whole persistence layer; the session registry lock gates all
runtime sessions.

**Fix:** Recover from poison deliberately (`lock().unwrap_or_else(|e| e.into_inner())`)
where the guarded invariant survives a panic, or `parking_lot::Mutex` (no poison)
plus catch-unwind boundaries around the critical sections. At minimum, document
which closures are panic-free and assert it.

### I3. Pending OAuth flows are never expired — secret retention + unbounded growth
**File:** `crates/posthaste-authority-runtime/src/oauth/flow_store.rs:84-92`.

`prune_terminal_oauth_states` retains **all** `Pending` flows unconditionally
(`StoredOAuthFlow::Pending(_) => true`); only `Completing`/`Completed` entries get
the 10-minute TTL. A `PendingOAuthFlow` holds `client_secret`, `pkce_verifier`,
and `nonce`. Any started-but-never-completed authorization (user closes the tab,
provider error, abandoned add-account) leaves that secret-bearing entry in the
`flows` map forever, and the map grows without bound across the process lifetime.

**Fix:** Stamp `Pending` with its creation time and prune it on the same TTL as
the terminal states (the authorization code is short-lived anyway, so a
~10-minute pending TTL is correct). Zeroize the secret on drop if practical.

---

## Suggested

### S1. Interrupted `draftCreate`/`draftUpdate` can be re-sent → duplicate provider drafts
**File:** `crates/posthaste-domain/src/service/outbox.rs:flush_account`
(`:~430-470`); `store/src/outbox.rs:list_flushable_operations` includes
`'inflight'`.

`flush_account` sets an op to `Inflight` *before* awaiting the provider call.
Only `Send` has send-once recovery ("found `inflight` → fail terminally"). For
`DraftCreate`/`DraftUpdate`, an op that actually reached the provider but whose
process died before settling is left `Inflight`; on the next flush it is re-listed
(`list_flushable_operations` matches `inflight`) and re-attempted. `save_draft`
with `None` creates a *new* provider draft each time, so the orphaned first draft
remains in the provider's Drafts mailbox (the alias only tracks the latest id).

**Fix:** Treat an `inflight` draft op the same way as `send` (don't blindly
re-attempt), or make draft creation idempotent via the stable
`X-Posthaste-Draft-Id` header — resolve an existing provider draft by that header
before creating, so a retry updates rather than duplicates.

### S2. Successful provider mutation with a failed readback silently drops authoritative state
**File:** `crates/posthaste-engine/src/live_mutation.rs:48-51` (`set_keywords`),
`:148-151` (`replace_mailboxes`).

After a successful `Email/set`, the readback is fetched with
`fetch_message_record(...).await.ok().map(...)` — a transient readback failure
becomes `outcome.message = None`. In `settle_message_operation`
(`outbox.rs:settle_message_operation`), a `None` readback removes/settles the op
as `Applied` with **no canonical write**, leaving the optimistic projection
unreconciled until the next full sync. The mutation succeeded remotely but the
local truth is never written; if the optimistic value and the real provider value
diverge (e.g. server normalized keywords), the UI shows stale state for a while.

**Fix:** Distinguish "set succeeded, readback failed transiently" from "no
readback supported (IMAP)". For JMAP, treat a readback failure after a successful
set as `Transient` so the settle retries, rather than dropping it.

### S3. cid-URL rewrite interpolates provider-controlled `attachment.id` into already-sanitized HTML
**File:** `crates/posthaste-api/src/api/messages/detail.rs:rewrite_inline_attachment_urls`
(`:~95-130`), called *after* `sanitize::sanitize_email_html`.

The rewrite does `html.replace("cid:<id>", "/v1/.../attachments/{attachment.id}")`
on the post-sanitization string. `attachment.id` originates from the provider/
message and is spliced in **without HTML/URL escaping** and **without
re-sanitizing**. If an `id` contained `"`/`<`/`>`, it would break out of the
`src`/`href` attribute and inject arbitrary markup, defeating the sanitizer's
guarantee. Currently mitigated to non-script-execution by the no-`allow-scripts`
iframe sandbox, so impact is markup/style injection only — hence "suggested", not
"important".

**Fix:** Percent-encode `attachment.id` for the URL (it's a path segment anyway),
or re-run the sanitizer after the rewrite, or assert the id matches a safe
`[A-Za-z0-9_-]` charset.

### S4. CSS `style` sanitization is substring-based and bypassable
**File:** `crates/posthaste-api/src/sanitize.rs:sanitize_style_value` (`:~145-160`).

Remote-content/tracking stripping rejects declarations containing the literal
`url(` or `expression(`. CSS allows escapes and token splitting that evade a
substring check (e.g. `\75rl(...)`, `url\28...\29`, case/whitespace variants the
filter doesn't fold). This is defense-in-depth for tracking pixels, not the XSS
boundary, but the comment claims it closes the remote-content vector, which it
does not close completely.

**Fix:** Parse declarations and reject any `url()`/`image-set()`/`-moz-binding`
function token after CSS unescaping, or rely on a frame-level CSP
`img-src`/`style-src` and document that the substring filter is best-effort.

### S5. `find_img_tag_start` off-by-one misses an `<img>` at the exact end of the document
**File:** `crates/posthaste-api/src/sanitize.rs:find_img_tag_start` (`:~180-195`).

`last_start = bytes.len().saturating_sub(4)` with `for index in 0..last_start`
never examines index `len-4`, so an `<img …>` whose `<img` begins exactly 4 bytes
from the end is not scanned for tracking-pixel stripping. Low impact (such a tag
has little room for a `src`), but the bound should be `0..=last_start` /
`0..bytes.len().saturating_sub(3)`.

### S6. Startup/serve panics on operator-config and post-bind errors
**File:** `crates/posthaste-api/src/serve.rs:95` (`origin.parse().expect("invalid CORS origin")`),
`:144-145` (bind/`local_addr` `expect`), `:174` (`axum::serve(...).await.expect("posthaste server failed")`).

A malformed configured CORS origin panics startup; the `axum::serve` future runs
inside a detached `tokio::spawn` whose `.expect` panics the task with nobody
joining it, so a serve failure silently kills the listener with only a panic in
logs. Prefer surfacing these as typed startup errors and logging a structured
fatal on the serve task rather than an unobserved panic.

### S7. `resolve_root_key` falls through silently on a malformed env key
**File:** `crates/posthaste-api/src/token.rs:108-118`.

A set-but-undecodable `POSTHASTE_MACAROON_ROOT_KEY` is silently ignored and
resolution proceeds to keyring/file — so an operator who fat-fingers the env var
gets a *different* (generated/persisted) key with no signal, and previously-minted
tokens silently stop verifying. The code comment acknowledges this. Recommend a
`tracing::warn!` (or hard fail) when the env var is present but unusable, rather
than a silent fallthrough.

---

## Notes / things checked and found sound

- **SQL construction** (`store/src/smart_mailboxes/*`, `query/*`): column names
  are fixed string literals selected by enum field; all user values bound via
  `params`/`SqlValue`. No injection. The `{expected}` integer in
  `field_compilers.rs:129` is a computed `0`/`1`, not user text.
- **Auth perimeter** (`auth/perimeter.rs`, `auth/middleware.rs`): Host allowlist
  runs before exemptions; Origin/Referer canonicalized via `url::Url` and
  fail-closed; bearer-only (no query-token); macaroon HMAC verified, then caveats
  enforced separately (401 vs 403 vs fail-closed-500 for unmapped scoped routes).
  `constant_time_eq` is correct.
- **OIDC/JWT** (`oauth/openid.rs`): algorithm pinned to RS256 (no alg-confusion);
  `kid` looked up in the trusted JWKS; manual `aud`/`iss`/`exp`/`nbf`/`nonce`/
  `email_verified` checks with bounded clock skew; JWKS cache TTL clamped.
- **OAuth state** (`oauth/flow_store.rs:begin_completion`): atomic
  Pending→Completing transition under the lock makes `state` single-use (CSRF/replay
  safe). (TTL gap is I3.)
- **Per-account sync serialization** (`supervisor/runtime.rs`): all sync work runs
  inline in one `tokio::select!` task per account; no concurrent flush → no
  in-process double-send. `trigger_account_sync` uses `reserve()` so triggers are
  never dropped between the `is_syncing` check and send. `RuntimeGeneration`
  fences stale writes from restarted tasks.
- **Store connection config** (`db/connection.rs`): WAL, `foreign_keys=ON`,
  `busy_timeout=5s`, `mmap_size=0` (deliberate, corruption-avoidance), single
  write connection behind a Mutex. Corruption is classified and auto-quarantined.
  `write_transaction` relies on rusqlite's drop-rollback (correct).
- **`session.event_task`** is aborted on `close_session`; view forwarders exit via
  weak-registry upgrade + session-lookup, so no obvious task leak (other than the
  state growth in I1).
- **`write_secure_file`** correctly re-asserts `0600` on overwrite.
- **domain crate** is essentially panic-free in non-test code (one invariant
  `unreachable!` at `service/cache/helpers.rs:53`).
