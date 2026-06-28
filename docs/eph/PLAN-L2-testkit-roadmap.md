---
scope: L2
summary: "Roadmap for the posthaste-testkit forward contracts: runtime-in-harness, view-settlement recorder, declarative fixtures, posthastectl headless driver"
modified: 2026-06-27
reviewed: 2026-06-27
lifecycle: ephemeral
type: PLAN
depends:
  - path: docs/testing/L1
  - path: docs/runtime/L1
  - path: docs/replication/L1
dependents:
  - path: docs/testing/L1
  - path: docs/eph/DESIGN-L2-render-flicker-tracker
---

# posthaste-testkit forward roadmap

Ephemeral plan. Folds back into `docs/testing/L1` as each slice lands, then this
doc is deleted. The `[::state planned]` / `[::state partial]` markers in
`docs/testing/L1` point here.

## Roadmap

### P0 — Spec migration (done 2026-06-26)

`docs/testing/{L0,L1}.md` created (superseding the legacy testing spec). The lab
domain migration is a separate slice.

### P1 — Shared testkit extraction (done 2026-06-26)

`posthaste-testkit` crate with `Harness`, `StalwartFixture`, and `paths`,
lifted from `stalwart_provider_parity`. `stalwart_provider_parity` and
`stalwart_identity_transport` migrated to consume it. No behavior change;
parity tests behave as before.

### P2 — Runtime-in-harness + view-settlement recorder (done 2026-06-26)

`Harness::with_runtime()` builds an in-process authority runtime against the
harness config root; `ViewSettlement` captures the ordered `RuntimeFrame` stream
a mutation settles through and asserts confirmed / recompute-present /
only-touched-view / session-seq-monotonic. First regression:
`keyword_toggle_settles_and_fires_notification_without_re_serving_the_view`.

**Superseded for mail-list views by option iii (2026-06-27):** the runtime no
longer recomputes + re-serves an evaluable mail-list view per event — the client
self-maintains it from the `message.updated` firehose
([single-source-view-membership]). The first regression now asserts the
option-iii contract (`assert_message_updated_notification` +
`assert_view_not_recomputed`); the recompute-present assertions
(`assert_view_recomputed_*`) remain valid for the still-server-recomputed view
kinds (detail, conversation, account).

Historical finding (pre-option-iii, characterized, not a bug): a `run_mutation`
keyword toggle on a live mock account recomputed the touched mail-list view
*twice* — optimistic (rev 2) then sync-confirmed (rev 3) — with a ~16-notification
fan-out between them. The notification fan-out breadth is still worth a future
profiling look.

### P3a — StalwartFixture::inject + live scenario (done 2026-06-26)

`StalwartFixture::inject(count)` SMTP-delivers a burst to the dev mailbox via a
single pooled `posthaste-imap` transport (added `send_smtp_messages`).
`RuntimeHarness::create_jmap_account(id, stalwart)` creates a JMAP account
pointed at the fixture and syncs it; `RuntimeHarness::watch_view` + `ViewWatch`
is the sync-driven drain that stays subscribed across an external action and
waits until a snapshot satisfies a predicate.

The live scenario (`twenty_injected_messages_converge_into_inbox_view`) injects
20 messages and asserts they converge into the inbox view. Earlier flagged as
"blocked on a runtime read-path gap" — the actual cause was a test-form
mismatch: the test queried `in:<account>/inbox` (bare name), but a live-synced
mailbox's `MailboxId` is namespaced (e.g. `jmap:<blob>`), never the bare
"inbox" the mock path seeds. The app queries by the actual id
(`apps/web/src/runtime/httpAdapter.ts` `scopeQuery`); the test now resolves the
inbox's real `MailboxId` via `list_mailboxes` to match. The read path was never
broken; the scenario passes (initial 4 seeded + 20 injected = 24, converged).

### P3b — Declarative fixtures (done 2026-06-26)

Typed `Fixture` / `FixtureAccount` / `FixtureMessage` TOML loaded via
`RuntimeHarness::load_fixture_toml` / `load_fixture`, driving
`create_mock_account` + a new typed `seed_messages_typed` (the tuple
`seed_messages` now delegates). `FixtureMessage` layers declared field
overrides (subject / from / preview / received_at / size / keywords /
thread_id / rfc_message_id) on a shared `default_message` baseline (single
source of truth). Only `driver = "mock"` is supported; `driver = "jmap"`
returns a typed `FixtureError::UnsupportedDriver` and lands with the live
read-path (the P3a gap). First test:
`declarative_fixture_loads_accounts_and_messages_into_the_view`.

### P3c — Mock Gmail IMAP fixture (increments 1–3 done 2026-06-27)

A hand-rolled TCP mock Gmail IMAP server (matching the `spawn_fake_smtp_server`
pattern) that advertises `X-GM-EXT-1` + `CONDSTORE` + `QRESYNC` + `IDLE` +
`UIDPLUS` + `ENABLE`, so the real `imap-client` negotiates them end-to-end.
This closes the gap the canned-data `gmail_labels` tests leave: those feed parsed
structures into the batch builders and never exercise capability negotiation,
FETCH parsing, or CONDSTORE/QRESYNC deltas at the protocol layer.

- Increment 1 (done): mock server core (greeting + `CAPABILITY` +
  `AUTHENTICATE PLAIN` + `LIST`) + a test asserting the real `imap-client`
  picks up `X-GM-EXT-1`/`CONDSTORE`/`QRESYNC`, `ProviderKind::Gmail` is
  selected, and Gmail mailboxes list with correct roles (`[Gmail]/All Mail` →
  Archive). Hand-written IMAP responses; lives in `posthaste-imap` discovery
  tests.
- Increment 2a (done): `SELECT`/`EXAMINE` (QRESYNC state: `UIDVALIDITY` +
  `HIGHESTMODSEQ` + `EXISTS`) + `UID FETCH` encoding `UID` + `RFC822.SIZE` +
  `RFC822.HEADER` (literal) + `X-GM-MSGID` + `X-GM-THRID` + `X-GM-LABELS` via
  the forked `imap-codec` `ResponseCodec`. A protocol-layer test drives the
  real `imap-client` → `examine_selected_mailbox` →
  `fetch_selected_mailbox_headers(fetch_gmail_metadata=true)` and asserts the
  `X-GM-LABELS` (incl. a quoted multi-word label `"Project Alpha"`) round-trip
  onto `ImapMappedHeader.gmail_labels`. Still lives in `posthaste-imap`
  discovery tests (not yet promoted to `posthaste-testkit`).
- Increment 2b (done): a stateful, multi-connection `GmailImapFixture` in
  `posthaste-testkit` (`src/gmail.rs`) that answers the full discovery + sync
  command set (`CAPABILITY`/`AUTHENTICATE`/`LIST`/`STATUS`/`EXAMINE`/`UID
  SEARCH`/`UID FETCH`) the real gateway drives across its discovery, then sync,
  connections. `RuntimeHarness::create_gmail_account` wires an `ImapSmtp`
  account at it, enables it (discovery), and syncs; a self-contained test
  (`tests/gmail_inbox_sync.rs`) asserts the seeded INBOX message lands in the
  store and surfaces in the `mailList` view (queried by the inbox's real
  namespaced `MailboxId`, like the JMAP live test). The fixture deliberately
  omits `IDLE` from its caps so the gateway skips the background push stream and
  the explicit `sync_account` drives a deterministic full-snapshot fetch; data
  model is one Gmail-labeled message in INBOX, other mailboxes empty.
  IDLE-push parity is later scope. (The single-shot protocol mock for
  increments 1/2a stays in `posthaste-imap` discovery tests — a different test
  level; `posthaste-imap` cannot depend on `posthaste-testkit`.)
- Increment 3 (done): `CONDSTORE`/`QRESYNC` deltas. `GmailImapFixture` now holds
  a shared, mutable `InboxModel`; `vanish_inbox_and_deliver(subject)` expunges
  the live message and delivers a new one, advancing `HIGHESTMODSEQ`.
  `RuntimeHarness::sync_account` re-syncs; the gateway's `STATUS` preflight sees
  the changed MODSEQ, then takes the QRESYNC delta path (`ENABLE QRESYNC` →
  `UID FETCH 1:* (CHANGEDSINCE <modseq> VANISHED)`). The mock answers with a
  changed `FETCH` + `VANISHED`, and the test asserts the view replaces the
  vanished message with the delivered one **and** that a `CHANGEDSINCE` fetch was
  actually issued (`changedsince_fetch_count`), proving the delta path — not a
  full re-snapshot — drove the convergence.

The forked `imap-codec`/`imap-types` (vendored under `vendor/`, built from the
GitHub rev — `vendor/` is reference-only) drive FETCH encoding. Quirks the
fixture had to work around, in increasing severity:

1. `Text` encodes raw bytes with no quoting — multi-word X-GM-LABELS must be
   pre-quoted in the mock.
2. **`ModSeq` encoder bug** (encoder-only, not a production risk): the encoder
   emits `MODSEQ <v>` but the decoder (correctly, per RFC 7162) requires
   `MODSEQ (<v>)`. Production only *decodes* (client side) and real Gmail sends
   the parenthesized form, so this never bites production — but our mock, which
   *encodes* responses, must splice `MODSEQ (<v>)` in by hand (the per-message
   MODSEQ is required: the stored HIGHESTMODSEQ watermark is derived from it, so
   without it the next sync never takes the delta path).
3. **`VANISHED` decoder bug (likely a real production defect)**: the fork's
   `response_data` only reaches its `VANISHED` arm through `message_data`, which
   requires a leading `<nz-number> SP`. A standard `* VANISHED (EARLIER) <uids>`
   (no number — per RFC 7162 *and* the fork's own encoder) therefore fails to
   decode and drops the connection ("stream error"). The mock works around it by
   emitting a dummy leading sequence number (`* 1 VANISHED (EARLIER) <uids>`),
   which the QRESYNC task ignores. **This means QRESYNC expunge sync against
   *real* Gmail likely errors on the first `VANISHED` response.** Verify against
   a real Gmail account (or extend the Stalwart parity test to drive QRESYNC),
   then fix the fork decoder to accept the number-less `VANISHED` form and bump
   the patched rev.

### P3d — Flicker diagnosis fixture (Layer A+B done 2026-06-27)

A runtime-layer flicker harness: drive a real mutation through the runtime while
a provider sync re-serves the view, capture the emitted `RuntimeFrame` stream
(**Layer A**, `RuntimeHarness::open_capture` → `FrameCapture::drain`), and replay
it through the *real* `posthaste_link_replica::EntityStore` reconciliation
(**Layer B**, `ReplicaProbe` — the same optimism/absorption code the browser runs
via WASM), recording the per-row render trajectory. `FlickerLog::assert_no_flicker`
catches any observable field reverting or a row disappearing-then-reappearing.
No browser, no `posthastectl` — the flicker-prone logic (EntityStore + runtime
frame emission) is all Rust.

- `tests/mutation_flicker.rs`: flag + mark-read on the mock driver's canned
  dataset, each interleaved with the driver's background poll re-serving the
  whole view. The mark-read case is the sharpest: the re-serve carries the
  message *unread* (stale) on every poll, yet the optimism holds (absorption-
  gated retire never retires against a base that lacks the effect). **Finding:**
  both are flicker-free — the runtime + EntityStore reconciliation does **not**
  flicker for flag/read during a stale re-serve. This narrows the residual
  nightly flicker to either (a) scenarios not yet reproduced here (real-async
  provider lag, delete row reappear, multi-message reorder, count/badge), or (b)
  **Layer C** — the TS `entityStoreAdapter` wiring / the legacy REST domain-cache
  path racing the WASM replica / React render timing.
- Layer C (done 2026-06-27, **root cause found**): `bun` tests driving the
  *real* WASM `EntityStore` (`apps/web/test/replicaAbsorptionRetire.test.ts`)
  caught the flicker without `posthastectl`/Playwright. Two coupled bugs in
  `link-replica`/`link-core` (see the `flicker-root-cause` memory):
  (1) **no staleness guard on `ingest_batch`** — a late provider-sync re-serve
  (snapshot pre-dating the mutation) clobbers a confirmed+retired op's base with
  nothing to fold it back → the row reverts → flicker (`true->false->true` once
  the next sync corrects it); (2) **absorption-retire broken for real
  projections** — `retire_absorbed` compares `MessageFoldState` keyword `Vec`s
  order-sensitively, but `fold_state_from_projection` keeps provider order while
  `apply_message_assertion` sorts via `BTreeSet`, so ops on any message carrying
  `$seen` (≥2 keywords) never retire (stuck-folding = protective). That makes the
  flicker intermittent: only messages where absorption works (single/sorted
  keywords, e.g. flagging an UNREAD message) retire and become vulnerable to (1).
  Fix (validated by rebuilding wasm with sorted keywords): sort keywords in
  `fold_state_from_projection` (or set-insensitive `MessageFoldState` eq) **and**
  add an ingest staleness guard — (1) is unmasked on more messages if only the
  absorption fix lands. Owned by views-stability; not landed here.
- Note: the existing fake-handle adapter test + the Rust unit tests miss both
  bugs (fake handle mirrors only minimal-keyword cases; unit tests use ≤1
  keyword). The gap was the *real adapter + real WASM + realistic projection*
  integration — exactly what the bun real-WASM probe exercises.
- Mock-driver notes surfaced: `AccountDriver::Mock` serves a canned dataset
  (`em-001`..`em-003` in `mb-inbox`/`mb-archive`) and re-serves it on every
  sync **plus a ~500ms background poll**; it rejects mutations on unknown
  message ids (`operation.settled: failed`). A direct `seed_messages` batch is
  wiped by the first mock sync.

### P4 — posthastectl headless driver

Dev/lab-only client of the daemon API: `health wait`, `settings get/patch`,
`accounts list`, `events wait`, `fixture load`, `state dump`. Composes with
`posthaste-lab` (lab invokes ctl as a runner). Distinct from lab: ctl drives a
running app's API; lab orchestrates runs and collects artifacts.

### P5 — Property tests + profiling in lab artifacts

`proptest` for the invertible-diff laws (`apply(apply(s,d), d.inverse()) == s`)
and replica convergence under reordered assertions. Wire `posthaste-bench`
artifacts into lab run manifests.

## Open findings (not blockers)

- `stalwart_provider_parity::stalwart_jmap_and_imap_sync_project_equivalent_fixture_messages`
  is currently red under `POSTHASTE_STALWART_INTEGRATION=1`: JMAP lists the
  trashed "Build failure on obsolete branch" (`.Deleted Items`); IMAP does not.
  Reproduces on `main` (pre-P1), so it is a pre-existing provider-parity bug
  hidden by the integration gate, not a testkit regression. The settlement
  recorder (P2) and a focused Deleted-Items parity case are the right tools to
  diagnose it.
