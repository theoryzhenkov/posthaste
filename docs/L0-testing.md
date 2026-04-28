---
scope: L0
summary: "Executable behavior contracts, red-first coverage, and provider parity test strategy"
modified: 2026-04-28
reviewed: 2026-04-28
depends:
  - path: README
  - path: docs/L0-providers
  - path: docs/L0-sync
  - path: docs/L0-api
  - path: docs/L0-ui
dependents:
  - path: docs/L1-sync
  - path: docs/L1-api
  - path: docs/L1-ui
---

# Testing domain -- L0

## Purpose

PostHaste tests are executable behavior contracts. They must prove that provider
observations, local reconciliation, API invalidation, and frontend state converge
to the expected user-visible model. Tests should not merely describe the current
implementation.

The default workflow for new coverage is red-first:

1. Identify expected behavior from a SPECial assertion, protocol rule, RFC, or
   provider behavior.
2. Write the smallest failing test that proves the behavior is missing or could
   regress.
3. Run that focused test and confirm it fails for the intended reason.
4. Implement the smallest behavior change needed to pass.
5. Run the focused suite plus the relevant crate/app checks.

If a proposed test passes before implementation, the author must either tighten
the case until it exercises a real missing edge or record why that behavior is
already covered and choose a different missing behavior. Characterization tests
of questionable current behavior are not accepted unless they are explicitly
temporary regression pinning before a refactor.

## Coverage model

Coverage is organized by behavioral boundary, not by file count.

- **Provider observation contracts** prove raw remote observations become the
  canonical local model. Examples: Gmail labels exposed as IMAP mailboxes,
  generic IMAP copied messages with shared RFC `Message-ID`, JMAP
  `cannotCalculateChanges`, IMAP MODSEQ and VANISHED observations.
- **Store reconciliation contracts** prove `SyncBatch` application is atomic,
  account-scoped, projection-safe, and event-complete.
- **Domain service contracts** prove mutations, automation, cache policy, and
  sync orchestration converge under retries, mismatches, and gateway failures.
- **API contracts** prove handler error mapping, pagination, SSE replay, and
  mutation responses match the frontend boundary.
- **Frontend state contracts** prove React Query invalidation, optimistic
  mutation visibility, surface history, keyboard routing, progress rendering,
  and full-screen overlay stacking behave from the user's perspective.
- **Live-provider parity contracts** prove JMAP and IMAP projections agree on the
  same seeded server fixture where a local real provider can be used.

## Provider observation matrix

Provider tests must enumerate observations before implementation details. Each
row should be represented by at least one focused unit/integration test unless a
row is explicitly out of scope for the current driver.

| Area | Observation | Expected local outcome |
|---|---|---|
| Gmail IMAP labels | Same logical message appears through Sent, Starred, and All Mail | One local message, multiple `ImapMessageLocation`s, unioned mailbox membership, one keyword set |
| Generic IMAP copies | Two mailboxes expose messages with the same RFC `Message-ID` but different UIDs | Separate local messages unless a provider-specific stable identity proves equality |
| IMAP flags | Remote flag changes without UIDNEXT or MESSAGES changes | Metadata refresh or MODSEQ/QRESYNC delta applies keyword changes; no skip based only on UID count |
| IMAP expunge | UID disappears from a selected mailbox | Local location is removed; message is deleted only when no provider-observed membership remains |
| IMAP UIDVALIDITY | UIDVALIDITY changes for a mailbox | Stored UID locations for that mailbox are invalidated by authoritative refresh |
| IMAP VANISHED | QRESYNC returns VANISHED UIDs | Matching local locations are deleted without pruning unrelated messages |
| JMAP changes | `cannotCalculateChanges` is returned | A full authoritative snapshot replaces the affected object set |
| Local mutation echo | A local keyword/mailbox mutation succeeds remotely | All visible mailbox/smart/tag views reflect the mutation before a manual sync is required |
| Remote mutation convergence | A mutation is applied remotely by another client | Push or poll eventually converges every local view without stale per-mailbox state |

## Test quality standard

Every new behavior test must be named by the behavior it proves, use
Arrange/Act/Assert structure, and avoid incidental implementation assertions.
When a test corresponds to a SPECial assertion, link it with a comment:

```rust
// spec: docs/L0-testing#provider-observation-contracts
```

```ts
// spec: docs/L0-testing#frontend-state-contracts
```

Subagents writing tests must include in their final report:

- the expected behavior source they used
- the red failure they observed, or why the behavior was already covered
- the files changed
- the focused command they ran
- remaining uncovered rows or risks

## Assertions

| ID | Sev. | Assertion |
|----|------|-----------|
| red-first-tests | MUST | New behavior coverage is developed red-first or explicitly justified as already covered before choosing another missing behavior |
| behavior-not-current-code | MUST | Tests assert expected protocol/domain behavior rather than snapshotting current implementation quirks |
| provider-observation-contracts | MUST | Provider adapters are tested against remote observation matrices before their output reaches the store |
| sync-convergence-contracts | MUST | Local and remote mutations are tested through sync convergence across every affected mailbox/smart/tag view |
| no-unsafe-skip | MUST | Sync optimizations have tests proving they cannot skip state changes they are responsible for observing |
| store-reconciliation-contracts | MUST | Store sync application tests prove account scoping, atomicity, event emission, projection refresh, and deletion semantics |
| api-boundary-contracts | MUST | API tests prove stable error codes, pagination cursors, SSE replay, and mutation response behavior |
| frontend-state-contracts | MUST | Frontend tests prove user-visible state transitions, not internal React component implementation details |
| spec-linked-coverage | SHOULD | Tests for SPECial assertions link to the assertion with a `spec:` comment |
| live-provider-parity | SHOULD | Real-server parity tests compare JMAP and IMAP projections for the same seeded message fixture |
