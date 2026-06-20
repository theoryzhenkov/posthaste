
### [testing domain] — DONE
- Created docs/testing/L0.md (non-stale format; preserves red-first workflow, behavior-boundary coverage model, provider observation matrix, spec-linked coverage convention, subagent reporting). Verified coverage homes against actual suites (imap/sync/tests, store/tests, domain/service/tests, server/tests, web/test, provider_parity.rs).
- depends declared: docs/backend/L1, docs/state/mail/L1, docs/api/L1, docs/client/L1, docs/ui/L1. No dependents (orchestrator handles back-links).
- Rewrote 18 `spec:` comments across 8 files: docs/L0-testing -> docs/testing/L0 (assertion-id fragments preserved):
  imap/sync/tests/{gmail_labels,delta,qresync}.rs, store/tests/imap_snapshots.rs,
  server/tests/api_boundary_contracts/cases.rs, server/tests/provider_parity.rs,
  domain/service/tests/message_mutation_retries.rs, web/test/{domainCache,surfaces}.test.ts.
- NOTE for orchestrator: stale docs/stale/L0-testing.md still present — delete in reconciliation pass.
