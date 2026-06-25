---
scope: L2
summary: "Public beta readiness audit across core mail, runtime/UI, release engineering, security, support, and documentation"
modified: 2026-06-20
reviewed: 2026-06-20
lifecycle: ephemeral
type: REPORT
depends:
  - path: docs/stale/L0-providers
  - path: docs/stale/L1-sync
  - path: docs/stale/L1-compose
  - path: docs/stale/L1-search
  - path: docs/runtime/adapter/L1
  - path: docs/backend/L1
  - path: docs/api/L1
  - path: docs/stale/L1-lab
  - path: docs/stale/L1-logging
---

# Public beta readiness audit

## 0. Update (2026-06-21)

Since this audit, three of the called-out gaps have landed:

- **Forward** is implemented end to end (`feat(compose): implement message forward`): backend `ReplyContext.forwarded_body` + frontend forwarded-block seeding and original-attachment re-attach. The forward-disabled UI gating is removed.
- **Attachment download** no longer re-fetches the whole IMAP message per attachment (`perf(attachments): serve cached attachment bytes without refetch`): cached raw bytes are served via `extract_cached_blob`.
- **Desktop auto-update** is wired (`tauri-plugin-updater` + GitHub Releases `latest.json`); activation requires adding the `TAURI_SIGNING_PRIVATE_KEY` CI secret. See `docs/eph/HOWTO-L2-desktop-auto-update`.

The rows below are the original audit state and are retained for context.

## 1. Executive summary

Posthaste is **not yet feature-complete for a broad public beta**. It is close enough for continued dogfood and a narrower private beta, but a public beta needs a small number of explicit product decisions and several hardening passes.

The strongest areas are local store performance, read/search responsiveness, event contract cleanup, account/runtime status surfaces, local API auth, and release-asset generation. The weakest beta areas are compose completeness, runtime/UI migration completeness, release artifact smoke/signing, public install/support docs, and live-provider confidence.

The recommended beta target should be: **safe for strangers to install as a secondary mail client, with clear beta caveats, manual update flow, and explicit provider/support scope.** Do not promise full offline mutation, full body search, forward, native IPC, or complete runtime-view architecture unless those are finished first.

## 2. Suggested beta gate

### 2.1 Must be done before public beta

| Gate | Status | Why it matters | Concrete next work |
| --- | --- | --- | --- |
| Public install/support path | Missing | Users cannot self-serve install, report bugs, or recover from failures. | Fill README/site install/support docs; document log locations, beta caveats, manual updates, and bug report template. |
| Release artifact smoke | Partial/missing | Installers can publish without launch/install verification. | Add release workflow smoke for Linux package; document/manual-gate macOS/Windows smoke; inspect release artifacts for no lab bridge. |
| macOS signing/notarization decision | Partial | Public macOS beta may hit Gatekeeper friction. | Either require Apple signing/notarization secrets and fail release without them, or explicitly label macOS builds as unsigned/ad-hoc beta. |
| Forward feature decision | Done (2026-06-21) | UI previously disabled forward. | Implemented: forwarded body + original-attachment re-attach; UI gating removed. |
| Runtime mail-list correctness if enabled | Partial/risky | Feature-gated runtime lists recompute only some updates and disable pagination/window extension. | Keep runtime mail-list feature flag off by default, or finish recompute triggers and pagination before beta. |
| Compose/reply polish | Partial | Sending mail is a core trust path. | Decide beta scope: send + basic reply only, or implement reply-all attribution and draft/autosave basics. |
| Provider matrix smoke | Unknown/partial | Public users will hit provider edge cases quickly. | Run and record live-provider smoke for Fastmail/Stalwart JMAP plus a limited IMAP/SMTP set. |
| Security/privacy release review | Partial | Public beta increases exposure. | Verify release auth defaults, no lab bridge, no token/body leaks in logs, account deletion local cleanup, devtools policy. |

### 2.2 Should be done before public beta

| Gate | Status | Why it matters | Concrete next work |
| --- | --- | --- | --- |
| Diagnostics/support bundle | Missing | Beta support should not require asking users to find JSONL logs manually. | Add or document “copy diagnostics”/log export; include app version, platform, account status, sanitized recent errors. |
| Public API/event docs | Partial | The event contract changed to coalesced `message.updated`. | Ensure docs, AsyncAPI, frontend vocabulary, and examples stay aligned; mark old topics removed. |
| Body search expectation | Partial | `body:` searches preview/metadata, not full fetched bodies. | Either implement body FTS or clearly document `body:` as preview/body-cache limited for beta. |
| Manual update policy | Missing | No updater found. | Document manual updates, changelog, rollback/uninstall; decide if in-app update is post-beta. |
| Account create recovery | Partial | Config/keyring writes are not transactional. | Add repair/GC check or document recovery steps for broken account records. |
| UI error/empty/loading polish | Partial | Users need understandable failure states. | Walk through account offline/auth failure, body fetch failure, send failure, sync failure, empty search. |

## 3. Area audit

### 3.1 Core mail and providers

| Capability | Status | Evidence summary | Beta decision |
| --- | --- | --- | --- |
| Account onboarding/config | Partial | Account create/patch/delete/verify routes and runtime mutations exist; secret/config crash-recovery remains future repair work. | Beta can proceed with docs/recovery guidance; add repair if time allows. |
| JMAP sync | Mostly ready | Live JMAP gateway, sync flow, lazy body/blob fetch, mutation paths, parity tests. | Primary recommended beta provider path. |
| IMAP/SMTP sync | Partial | IMAP sync, QRESYNC/CONDSTORE/full snapshot/Gmail label tests exist; destructive semantics are provider-sensitive. | Include as beta-supported only for a documented provider set after live smoke. |
| SMTP send | Partial | SMTP send and optional Sent append exist; Sent append failures warn. | Acceptable with caveat; verify Sent behavior per provider. |
| Read/list/search/detail | Mostly ready | Local projections, FTS search, smart/mailbox/source list paths are implemented and recently optimized. | Ready for beta, except body search expectation. |
| Body/detail/attachments | Partial | Lazy body fetch and attachment download exist; unavailable gateway returns partial detail. | Acceptable if UI communicates loading/failure clearly. |
| Compose/send | Partial | Send validation and JMAP/SMTP send exist; draft/autosave lifecycle not evident. | Beta can support basic send if drafts/autosave are de-scoped. |
| Reply | Partial | Reply context and headers exist; reply-all/attribution incomplete. | Support basic reply; de-scope polished reply-all unless implemented. |
| Forward | Missing | UI explicitly disables forward; backend lacks forwarded body/attachment reattach. | Hide/de-scope or implement before beta. |
| Mutations | Partial/mostly ready | Read/flag/tags/move/delete exist over JMAP and IMAP; IMAP destructive fallback is risky. | Ready for JMAP; IMAP needs live-provider smoke and copy wording. |

Highest core-mail blockers: **forward**, **compose/reply scope**, **live-provider matrix**, **body-search promise**.

### 3.2 Runtime and UI integration

| Capability | Status | Evidence summary | Beta decision |
| --- | --- | --- | --- |
| Bundled transport | Partial/ready for loopback | Desktop injects loopback runtime mode/port/token; native adapter unsupported. | Ship loopback mode; do not promise native IPC. |
| Runtime sessions/frames | Partial | Shared frame stream exists; list view and notifications are tested. | Good foundation, but not the only UI state path. |
| Runtime views | Partial | Backend view registry supports mail-list only; detail/conversation/compose/settings are target-only. | Keep target architecture internal; beta can use HTTP-backed facades. |
| Runtime mail-list feature flag | Risky | Runtime list path is feature-gated; pagination disabled; recompute triggers keyword-focused. | Keep off by default unless recompute and pagination are completed. |
| Renderer event handling | Partial | Domain cache handlers migrated to coalesced `message.updated`; legacy refetch remains. | Acceptable for beta if tested against real event streams. |
| Mutation settlement UX | Partial/missing | Backend emits settlement for setKeywords; renderer mostly ignores settlement frames and local optimistic runner owns rollback. | Do not sell runtime-backed undo/settlement as complete. |
| Compose UI | Partial | New/reply compose exists; forward disabled. | Beta scope must be explicit. |

Highest UI/runtime blockers: **runtime list flag default**, **mutation settlement UX**, **forward disabled**, **non-mail-list runtime views incomplete**.

### 3.3 Release engineering and public packaging

| Capability | Status | Evidence summary | Beta decision |
| --- | --- | --- | --- |
| CI baseline | Partial | Rust/web/site/docs/lab smoke run on Ubuntu. | Good baseline, but not enough for release artifacts. |
| Release artifacts | Partial | Workflows build desktop bundles and browser-localhost archives. | Good foundation. |
| Signing/notarization | Partial/blocker candidate | Apple signing/notarization optional; ad-hoc fallback. | Decide fail-closed vs explicit unsigned beta. |
| Artifact smoke | Missing/partial | Optional desktop lab smoke exists; not release-gated. | Add at least Linux packaged launch smoke before public beta. |
| Release integrity | Partial | Checksums/GPG/cosign/attestations are wired but some are best-effort. | Decide which integrity checks are mandatory. |
| Website releases page | Partial | Release page and generated content exist; beta warning exists. | Needs support/install/update copy. |
| In-app updater | Done (2026-06-21) | Now wired via tauri-plugin-updater + GitHub Releases latest.json. | Activate by adding the TAURI_SIGNING_PRIVATE_KEY CI secret; see HOWTO-L2-desktop-auto-update. |
| Release runbook | Missing/partial | Scripts exist, no single operator checklist found. | Write runbook before public beta. |

Highest release blockers: **artifact smoke**, **macOS signing/notarization stance**, **install/support docs**, **release runbook**.

### 3.4 Security, privacy, support, and observability

| Capability | Status | Evidence summary | Beta decision |
| --- | --- | --- | --- |
| API auth/perimeter | Ready | Auth default-on, loopback default bind, host/origin allowlist, macaroon tokens. | Ready; document dangerous overrides. |
| Credential storage | Mostly ready | Provider secrets via OS keyring/env; client tokens in desktop keyring; tests reject token persistence. | Ready; verify platform-specific keyring behavior in release smoke. |
| Lab/debug gating | Partial/mostly ready | E2E bridge feature-gated/Linux-only/private socket; devtools setting/build feature exists. | Add release artifact inspection/smoke. |
| Logging privacy | Partial | Backend logs avoid query/auth; frontend logger can forward arbitrary message/error/raw fields. | Review raw/error logging before beta. |
| Telemetry | Ready | No remote telemetry; no-telemetry guard exists. | Good beta posture. |
| Diagnostics/support | Missing/partial | Local logs/lab tools exist; no user-facing diagnostic export found. | Add docs or UI before public beta. |
| Account deletion cleanup | Ready for local data | Local config/source data/managed secrets removed and tested. | Ensure copy does not imply remote provider deletion. |
| User-visible status/errors | Partial/mostly ready | Account status/error/progress surfaces exist; no global support/error center found. | Acceptable if support docs explain where to look. |

Highest security/support blockers: **diagnostic/support workflow**, **log privacy review**, **release-mode lab/devtools proof**.

## 4. Recommended beta scope

### 4.1 Public beta should promise

- Local-first mail reading from synced accounts.
- JMAP-first account support, with IMAP/SMTP as beta/limited-provider support after matrix smoke.
- Inbox/mailbox browsing, smart/search views, message detail, lazy bodies/attachments.
- Basic send and reply.
- Read/unread, flag, move/archive/trash/delete with provider caveats.
- Local API for integrations, with auth and beta-stability caveats.
- Manual updates from the releases page.

### 4.2 Public beta should not promise yet

- Forwarding, unless implemented.
- Full body search across all fetched mail, unless implemented/documented precisely.
- Offline mutation queue.
- Complete native IPC runtime.
- Complete runtime session/view architecture for all UI state.
- In-app updates.
- Enterprise-grade provider breadth.
- Zero-friction signed/notarized macOS install unless release signing is hardened.

## 5. Proposed work plan

### Phase A — beta truth and docs (1 short pass)

1. Define beta provider scope: JMAP primary, named IMAP/SMTP providers only after smoke.
2. Update README install/support sections.
3. Update website beta copy: caveats, manual update, supported providers, bug-report path.
4. Write release runbook: tag, build, artifact smoke, signing/notarization, site check, rollback.
5. Decide forward/body-search/drafts beta scope and hide or label incomplete UI.

### Phase B — release hardening

1. Add release artifact smoke for at least Linux desktop artifact.
2. Add release-mode check proving no e2e lab bridge/debug mutation endpoints.
3. Decide macOS signing fail-closed vs unsigned-beta warning.
4. Add support diagnostics/log-export or a documented manual diagnostic bundle.
5. Run live-provider matrix and record results.

### Phase C — product blockers

1. Either implement forward or remove/hide forward affordance in beta.
2. Polish compose/reply failure states and send-result behavior.
3. Keep runtime mail-list feature off, or finish runtime view invalidation for move/delete/new mail and pagination.
4. Review frontend logging payloads to prevent body/token leaks.
5. Validate account deletion/re-add and account secret repair scenarios.

## 6. Suggested issue list

### Blocker

- `beta: write install/support/update docs and site copy`
- `beta: add release artifact smoke gate`
- `beta: decide and enforce macOS signing/notarization policy`
- `beta: define supported provider matrix and run live smoke`
- `beta: de-scope or implement message forward`
- `beta: keep runtime mail-list flag off or finish invalidation+pagination`

### High priority

- `beta: add diagnostics/log export or support bundle docs`
- `beta: audit frontend/backend logs for message body/token leakage`
- `beta: document body-search limitations or implement body FTS`
- `beta: compose/reply/send error-state polish`
- `beta: account create/delete/re-add recovery smoke`

### Nice before beta

- `beta: in-app update check or update notification`
- `beta: account repair/GC for config-secret partial writes`
- `beta: runtime mutation settlement UI for pending/failed commands`
- `beta: release runbook automation for checksums/signatures/site verification`

## 7. Bottom line

Posthaste is **backend-capable and dogfood-release capable**, but not yet **public-beta complete**. The fastest path to public beta is not more backend optimization. It is:

1. constrain the beta promise,
2. harden release/install/support,
3. smoke real providers,
4. explicitly de-scope incomplete UI/runtime features,
5. finish the few user-visible blockers that remain.
