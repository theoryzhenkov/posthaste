# LOC-Reduction Audit — posthaste-imap / -engine / -lab / -bench + vendor/

Scope: `crates/posthaste-{imap,engine,lab,bench}` (~16.6k LOC Rust) and the
`vendor/imap-codec` + `vendor/imap-types` forks (~28k LOC). Goal is **lines of
code reduction for AI-context fit**, not correctness. No source files were edited.
Evidence date: 2026-06-29, workspace `/.workspaces/reduce-loc`.

## Headline

The crates themselves are **genuinely lean** — small modules, sane test density,
no `todo!`/dead feature flags, shared domain helpers already factored. The
in-scope crate savings are modest (~170 LOC). **The entire LOC win is `vendor/`:
28,167 LOC of Rust that is not git-tracked, excluded from the workspace, and not
the dependency the build actually resolves.** That one exclusion is ~99.4% of the
total.

LOC baseline (incl. tests):
| crate | total | test LOC | test share |
|---|---|---|---|
| posthaste-imap | 8,516 | 3,189 | 37% |
| posthaste-engine | 4,602 | 924 | 20% |
| posthaste-lab | 2,481 | 710 | 29% |
| posthaste-bench | 1,047 | 0 (bench harness) | — |
| **vendor/** (codec+types) | **28,167** | 1,696 | 6% |

---

## Findings

| ID | category | file:line(s) | EST_LOC_SAVED | risk / behavior-change | how |
|----|----------|--------------|---------------|------------------------|-----|
| F1 | VENDOR (dead) | `vendor/imap-codec/**`, `vendor/imap-types/**` | **28,167** | low / **n** | Delete the local vendored fork copy; it is unused by the build and not even tracked by git. |
| F2 | DEAD (wrappers) | `crates/posthaste-imap/src/fetch/headers.rs:4-113`, `crates/posthaste-imap/src/fetch/changed_since.rs:13-39` | ~80 | med / n | Remove the 4 standalone `connect+fetch` public wrappers; production uses the `_with_client` variants only. |
| F3 | DEAD (unused fn) | `crates/posthaste-imap/src/mailbox.rs:18-42` | ~25 | low / n | Delete `examine_imap_mailbox` — no caller anywhere; only the lib re-export references it. |
| F4 | BOILERPLATE | `crates/posthaste-imap/src/lib.rs:24-66` | ~30 | low-med / n | Downgrade the ~44 internal-only `pub` symbols to `pub(crate)` and drop them from the `pub use` block (only 6 are used outside the crate). |
| F5 | DUP | `crates/posthaste-engine/src/compose.rs:11-25` vs `crates/posthaste-imap/src/smtp/message.rs:103-118` | ~20 | low / n | Move the byte-identical markdown→HTML renderer to `posthaste-domain`; both crates call it. |
| F6 | DUP | `crates/posthaste-engine/src/compose.rs:55-65` vs `crates/posthaste-imap/src/compose.rs:77-87` | ~12 | low / n | Move the identical `prefix_subject` to `posthaste-domain` next to `recipients_to_header`/`format_forwarded_body`. |
| F7 | BOILERPLATE | `crates/posthaste-imap/src/fetch/headers.rs:25-34,99-108`, `changed_since.rs` | ~20 | low / n | If F2's wrappers are kept, extract the repeated `refresh_capabilities + normalize_imap_capabilities + modseq/gmail` block into one helper (subsumed by F2 if removed). |

**TOTAL est LOC saved: ~28,354** (F1 alone = 28,167; in-scope crates ≈ 187).

---

## Detail & evidence

### F1 — `vendor/` is fully dead weight (28,167 LOC) — the only material win

The vendored forks are **not the dependency that builds**:

- `Cargo.toml:25` — `exclude = [... "vendor/imap-codec", "vendor/imap-types"]`
  (not workspace members).
- `Cargo.toml:109-113` — `[patch.crates-io]` points `imap-codec`/`imap-types` at
  `git+https://github.com/theoryzhenkov/imap-codec.git?rev=2d19dd17…`.
- `Cargo.lock` — `source = "git+https://…?rev=2d19dd17…"`. The resolved source is
  the GitHub fork, **not** `vendor/`.
- `.cargo/config.toml` — no `[source]` `replace-with` and no `paths` override that
  would redirect the git URL to the local copy.
- `git ls-files vendor/` → **0 files** (the tree is untracked; `.gitignore` only
  ignores `vendor/*/Cargo.lock` and `vendor/*/target/`).

So `vendor/` is a *local working copy* of a fork that is published and pinned by
rev on GitHub. Breakdown: `imap-codec` src 13,968 + tests 1,435 + examples 350;
`imap-types` src 12,132 + tests 261 + examples 21 = **28,167 LOC / ~1.2 MB**.

- **For AI context:** exclude `vendor/` entirely today — it is never read by the
  build and duplicates an external pin.
- **For the repo:** it can be deleted outright (zero build impact). If a local
  copy is wanted for offline/audit reasons, keep it but add the short
  `vendor/*/FORK.md` divergence note already recommended in `review-tests-docs.md
  §3.3` so the delta vs upstream alpha is auditable. Either way it should not be
  in AI context.

Risk **low**, behavior-change **n**: the build does not use it.

### F2 — Dead standalone IMAP fetch wrappers (~80 LOC)

Four `pub` functions open their own connection, refresh capabilities, then
delegate to a `_with_client` variant. The production gateway path uses only the
`_with_client` variants against a pooled client (`gateway/execution.rs:270` →
`fetch_mailbox_headers_after_uid_with_client`). The standalone wrappers have **no
non-test, non-re-export caller**:

- `fetch_mailbox_header_records` (headers.rs:4) — callers: re-exports only.
- `fetch_mailbox_header_snapshot` (headers.rs:21) — caller: only
  `fetch_mailbox_header_records` (itself dead) + tests.
- `fetch_mailbox_headers_after_uid` (headers.rs:89) — callers: re-exports + tests.
- `fetch_mailbox_changed_since_snapshot` (changed_since.rs:13) — re-exports only.

Each is the ~13-line connect/capability block + a delegate call. Risk **med**
because their dedicated tests in `fetch/tests.rs`/`changed_since` exercise them
through a mock server; removing the wrappers means repointing those tests at the
`_with_client` variants (or dropping the duplicate coverage). No production
behavior changes (no live caller).

### F3 — `examine_imap_mailbox` is unreferenced (~25 LOC)

`crates/posthaste-imap/src/mailbox.rs:18-42`. Full-tree grep shows it referenced
only by its own definition and `lib.rs:44`. `body.rs:37` and
`mutation/validation.rs:30` call `client.examine(...)`/`client.select(...)`
directly and then `selected_mailbox_from_examine` (which *is* live), bypassing
this wrapper. Safe to delete the function + its re-export. Low risk / n.

### F4 — IMAP crate over-exports (~30 LOC of re-exports)

`lib.rs:24-66` re-exports ~50 symbols. Actual external consumers (whole repo,
`crates/` + `apps/`):

```
imap_body_from_raw_mime, ImapConnectionConfig, imap_mailbox_id,
LiveImapSmtpGateway, send_smtp_messages, SmtpConnectionConfig
```

— **6 of ~50**. The other ~44 (`*_by_location`, `fetch_mailbox_*`, `imap_*_sync_batch`,
`fetched_*`, etc.) are used only within the crate. Downgrading them to
`pub(crate)` and trimming the `pub use` block removes ~30 lines of re-export and,
more usefully, lets `cargo` surface any now-internally-dead functions (F2/F3 were
found by hand; this would make the rest fall out automatically). Risk **low-med**:
verify no `posthaste-server` integration test imports one of the trimmed symbols
(grep across `crates/`+`apps/` already shows none). Engine is already tight — only
3 symbols (`connect_jmap_client`, `LiveJmapGateway`, `MockJmapGateway`) are used
externally and its `lib.rs` only re-exports a handful.

### F5 / F6 — Compose helpers duplicated across imap↔engine (~32 LOC)

- **render markdown:** `engine/compose.rs:11-25` (`render_markdown`) and
  `imap/smtp/message.rs:103-118` (`render_smtp_markdown`) are byte-identical
  (same `Options`, same image/HTML `filter_map`, same `<!DOCTYPE…>` skeleton).
  Move one to `posthaste-domain` (where `format_forwarded_body`/`recipients_to_header`
  already live) and have both crates call it. ~20 LOC.
- **prefix_subject:** identical in `engine/compose.rs:55-65` and
  `imap/compose.rs:77-87`. Move to domain alongside the existing compose helpers.
  ~12 LOC.

Both are pure functions already exercised by tests on both sides; low risk / n.

---

## Things that look reducible but are NOT (deliberately not counted)

- **`engine/src/mock.rs` (464 LOC)** — largest in-scope file, but it is the shared
  `MailGateway` test double consumed by `posthaste-authority-runtime` tests and
  `posthaste-bench` (`supervisor.rs`, `supervisor/connection.rs`,
  `authority_runtime_handle.rs`, `runtime_workload`). It is compiled into the
  engine lib unconditionally; the only *structural* improvement (no LOC win) would
  be gating it behind a `test-support` feature so production builds drop it. Keep.
- **`bench/workloads.rs` (379) vs `runtime_workload.rs` (345)** — not duplication;
  they profile different tiers (store floor vs full runtime view-recompute path),
  both single-source-of-truth for the profile binary + criterion + iai gate. Keep.
- **`engine/sync/cursor.rs` vs `imap/sync/cursors.rs`** — look like a pair but are
  protocol-specific (JMAP state-string JSON vs IMAP content fingerprint); no shared
  logic. Keep.
- **Tests** — imap is 37% tests but density is reasonable (e.g. gateway/tests.rs =
  13 tests/324 LOC, discovery/tests.rs = 6/349) and `MessageRecord` literals appear
  only 8× in scope (domain already has `sample_message_record` fixtures). No
  test-bloat finding worth the coverage risk; don't cut tests for LOC.
- **`push_sse.rs`/`push_ws.rs` + `push_common.rs`** — already DRY'd; the shared
  conversion lives in `push_common`, transports are thin. Keep.

---

## Top 5 by LOC ÷ risk

1. **F1 — delete/exclude `vendor/`** — 28,167 LOC, low risk, zero build impact.
   This is the whole game; do it first. (Add a `FORK.md` if a local copy is kept.)
2. **F2 — remove the 4 dead IMAP fetch wrappers** — ~80 LOC, med risk (test
   rewiring), no production caller.
3. **F4 — trim IMAP `pub` surface to `pub(crate)`** — ~30 LOC + unlocks automatic
   dead-code detection for the rest of the crate, low-med risk.
4. **F5 — dedupe the markdown renderer into domain** — ~20 LOC, low risk.
5. **F3 — delete `examine_imap_mailbox`** — ~25 LOC, low risk, fully unreferenced.

(F6 prefix_subject ~12 LOC and F7 ~20 LOC are low-risk mop-up, mostly subsumed by
F4/F2.)
