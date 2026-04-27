---
scope: L1
type: ISSUE
lifecycle: ephemeral
summary: "Local issue triage before public GitHub Issues"
modified: 2026-04-27
reviewed: 2026-04-27
depends:
  - path: docs/L0-providers
  - path: docs/L1-api
---

# Local issue triage

This file tracks launch-blocking or near-launch issues before public GitHub
Issues are enabled. Keep reports short and actionable. Migrate open items to
GitHub Issues when the public tracker is ready, then archive or delete this
ephemeral log.

## Open

### PH-001: IMAP initial sync progress is opaque

- Status: open
- Severity: medium
- Area: IMAP/OAuth
- Observed: During first Gmail OAuth/IMAP sync, the account remains `syncing`
  while logs mostly show low-level `imap_next::fragment` read/write spans.
- Expected: The UI or logs should expose a clear current phase, mailbox name,
  message count, and completion/error state.
- Notes: OAuth token storage and TLS IMAP connection succeeded. The sync did
  eventually load data, but progress was hard to inspect while it was running.

## Closed

### PH-002: OAuth account editor exposes password and base URL fields

- Status: closed
- Severity: medium
- Area: Settings accounts UI
- Observed: A Google account created through OAuth still showed editable Base
  URL and Password fields, and did not clearly indicate Google/OAuth.
- Expected: Provider OAuth accounts should display provider/auth details and
  hide manually edited transport credentials.
- Resolution: OAuth accounts now show read-only connection details and omit
  manual server/password controls. Account overviews now expose a backend-owned
  `connection.kind` variant for manual credentials versus managed OAuth.
