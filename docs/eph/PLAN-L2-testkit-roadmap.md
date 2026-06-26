---
scope: L2
summary: "Roadmap for the posthaste-testkit forward contracts: runtime-in-harness, view-settlement recorder, declarative fixtures, posthastectl headless driver"
modified: 2026-06-26
reviewed: 2026-06-26
lifecycle: ephemeral
type: PLAN
depends:
  - path: docs/testing/L1
  - path: docs/runtime/L1
  - path: docs/replication/L1
dependents:
  - path: docs/testing/L1
---

# posthaste-testkit forward roadmap

Ephemeral plan. Folds back into `docs/testing/L1` as each slice lands, then this
doc is deleted. The `[::state planned]` / `[::state partial]` markers in
`docs/testing/L1` point here.

## Roadmap

### P0 — Spec migration (done 2026-06-26)

`docs/testing/{L0,L1}.md` created; `docs/stale/L0-testing` superseded. The lab
domain (`docs/stale/L0-lab`, `L1-lab`) migration is a separate slice.

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
`keyword_toggle_settles_and_recomputes_the_touched_view`.

Finding (characterized, not a bug): a `run_mutation` keyword toggle on a live
mock account recomputes the touched view *twice* — optimistic (rev 2, from the
mutation's local event) then sync-confirmed (rev 3, after the supervisor syncs)
— with a ~16-notification fan-out between them. This is expected local-first
two-phase behavior, so the first test asserts `at_least_once`, not
`exactly_once` (the latter is reserved for no-follow-up-sync scenarios). The
notification fan-out breadth is worth a future profiling look.

### P3 — Declarative fixtures + StalwartFixture injection

Typed `fixture.*` (accounts, mailboxes, messages, provider state) loaded from
TOML. `StalwartFixture::inject` for live SMTP/LMTP message bursts (e.g. the
20-messages-to-inbox scenario) so the app's real sync path observes them rather
than being bypassed.

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
