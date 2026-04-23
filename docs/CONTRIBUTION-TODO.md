---
scope: L0
summary: "Junior developer onboarding tasks — frontend, backend, and project infrastructure"
modified: 2026-04-02
reviewed: 2026-04-23
depends:
  - path: README
  - path: docs/L0-ui
  - path: docs/L0-api
---

# Contribution TODO

Concrete, well-scoped tasks for new contributors. Each task is completable in 1--5 hours. Frontend tasks require basic React/TypeScript. Backend tasks require zero prior Rust experience.

Two junior developers are joining. They have **no Rust experience** and **some frontend experience**.

---

## Frontend

### 1. Add loading skeletons to message detail panel

**Difficulty**: Easy

`MessageDetail.tsx` shows plain "Loading..." text while fetching. Replace with skeleton loaders using Tailwind `animate-pulse` that match the layout structure (subject, from/date, tags, body frame).

**Files**: `web/src/components/MessageDetail.tsx`
**Depends on**: nothing
**Skills learned**: React Query loading states, Tailwind animations, component composition

---

### 2. Add aria-labels and keyboard shortcut hints to toolbar buttons

**Difficulty**: Easy

Toolbar buttons (Archive, Trash, Flag) in `App.tsx` have `title` attributes but lack `aria-label` for screen readers. Update titles to show keyboard shortcuts (e.g. "Archive (e)", "Trash (#)").

**Files**: `web/src/App.tsx`
**Depends on**: nothing
**Skills learned**: Web accessibility (ARIA), semantic HTML

---

### 3. Create error boundary component

**Difficulty**: Medium

No top-level error boundary exists. Create `ErrorBoundary.tsx` that catches React rendering errors, shows a user-friendly UI with error details in dev mode only, and provides a "Reload" button.

**Files**: new `web/src/components/ErrorBoundary.tsx`, `web/src/App.tsx`
**Depends on**: nothing
**Skills learned**: React error boundaries, error recovery UX

---

### 4. Add empty state illustrations

**Difficulty**: Easy

Several components show plain text for empty states ("No threads in this view", "Select a message"). Replace with lucide-react icons + descriptive message + next-step hints.

**Files**: `web/src/components/MessageList.tsx`, `web/src/components/MessageDetail.tsx`, `web/src/components/Sidebar.tsx`
**Depends on**: nothing
**Skills learned**: UI/UX polish, icon usage

---

### 5. Add copy-to-clipboard for email addresses

**Difficulty**: Easy

In MessageDetail, the sender email is displayed but not copyable. Add a small copy icon button with tooltip and brief "Copied!" feedback using `navigator.clipboard.writeText()`.

**Files**: `web/src/components/MessageDetail.tsx`
**Depends on**: nothing
**Skills learned**: Clipboard API, micro-interactions

---

### 6. Add unread count to browser tab title

**Difficulty**: Easy

Browser tab just says "PostHaste". Create a `useDocumentTitle()` hook that computes total unread from sidebar data and updates `document.title` (e.g. "PostHaste (3)").

**Files**: new `web/src/hooks/useDocumentTitle.ts`, `web/src/App.tsx`
**Depends on**: nothing
**Skills learned**: Document API, custom hooks, side effects

---

### 7. Improve message row typography hierarchy

**Difficulty**: Easy

`MessageRow` uses `font-semibold` for unread but overall hierarchy (sender, subject, preview) could be sharper. Review font weights, padding, and gap for better readability.

**Files**: `web/src/components/MessageRow.tsx`
**Depends on**: nothing
**Skills learned**: Typography, Tailwind, visual hierarchy

---

### 8. Add confirmation dialog for destructive actions

**Difficulty**: Medium

Trashing messages via keyboard shortcut (#) has no confirmation. Create a reusable `ConfirmDialog.tsx` with Escape-to-cancel and message preview.

**Files**: new `web/src/components/ConfirmDialog.tsx`, `web/src/hooks/useEmailActions.ts`, `web/src/components/MessageList.tsx`
**Depends on**: nothing
**Skills learned**: Dialog patterns, keyboard handling, state management

---

### 9. Polish search bar clear button and focus management

**Difficulty**: Easy

Search bar in `App.tsx` expands on focus but the clear button is subtle. Improve hover state, add "Escape to clear" hint, and consider input debounce.

**Files**: `web/src/App.tsx`
**Depends on**: nothing
**Skills learned**: UX polish, keyboard shortcuts, input handling

---

### 10. Create `formatBytes()` utility with tests

**Difficulty**: Easy

Messages have size info but it is never displayed. Create `web/src/utils/formatBytes.ts` with proper unit formatting (B, KB, MB, GB) and write Vitest tests for it.

**Files**: new `web/src/utils/formatBytes.ts`, new `web/src/utils/__tests__/formatBytes.test.ts`
**Depends on**: Task 25 (testing framework)
**Skills learned**: Utility functions, testing patterns

---

### 11. Add focus-visible styles to all interactive elements

**Difficulty**: Medium

Keyboard navigation needs visible focus indicators. Add `focus-visible:ring-1 focus-visible:ring-ring` to all buttons and clickable elements. Test tab order (sidebar -> list -> detail).

**Files**: `web/src/App.tsx`, `web/src/components/Sidebar.tsx`, `web/src/components/MessageList.tsx`, others
**Depends on**: nothing
**Skills learned**: Accessibility, Tailwind focus states, keyboard navigation

---

### 12. Add loading skeleton to settings panel

**Difficulty**: Medium

`SettingsPanel` loads settings and smart mailboxes but has no skeleton. Add progressive loading UI to avoid flash of empty state.

**Files**: `web/src/components/SettingsPanel.tsx`
**Depends on**: Task 1 (pattern established)
**Skills learned**: React Query states, progressive loading

---

### 13. Add tooltip to unread indicator dot

**Difficulty**: Easy

The small blue dot in `MessageRow` for unread status is not immediately obvious. Add a hover tooltip "Unread message".

**Files**: `web/src/components/MessageRow.tsx`
**Depends on**: nothing
**Skills learned**: Tooltips, hover states

---

### 14. Extract message list virtualization constants to config

**Difficulty**: Easy

`MessageList.tsx` has magic numbers (PAGE_SIZE, ROW_HEIGHT, OVERSCAN_ROWS). Extract to `web/src/config/messageListConfig.ts` with comments explaining each.

**Files**: new `web/src/config/messageListConfig.ts`, `web/src/components/MessageList.tsx`
**Depends on**: nothing
**Skills learned**: Code organization, configuration management

---

## Backend

### 15. Extract error code constants

**Difficulty**: Easy

API error codes ("NOT_FOUND", "INVALID_REQUEST") are scattered as string literals in `account_support.rs` and `cursor_support.rs`. Create `error_codes.rs` with named constants and update handlers.

**Files**: new `crates/posthaste-server/src/error_codes.rs`, `crates/posthaste-server/src/api/account_support.rs`, `crates/posthaste-server/src/api/cursor_support.rs`
**Depends on**: nothing
**Skills learned**: Rust modules, constants, DRY principle

---

### 16. Add request/response logging middleware

**Difficulty**: Medium

The API has no structured logging. Add tracing middleware that logs HTTP method, path, status code, and duration for every request.

**Files**: new `crates/posthaste-server/src/logging.rs`, `crates/posthaste-server/src/main.rs`
**Depends on**: nothing
**Skills learned**: Rust logging (tracing), middleware, observability

---

### 17. Add health check endpoint

**Difficulty**: Easy

Add `GET /v1/health` that returns `{"status":"ok"}` (200) or 503 if the database is unavailable. Useful for monitoring and deployment.

**Files**: `crates/posthaste-server/src/api.rs`, `crates/posthaste-server/src/main.rs`
**Depends on**: nothing
**Skills learned**: Axum routing, async functions, HTTP status codes

---

### 18. Add rate limiting to API routes

**Difficulty**: Medium

The API has no rate limiting. Add `tower-governor` or similar middleware with 429 responses when limits are exceeded.

**Files**: `crates/posthaste-server/src/main.rs`
**Depends on**: nothing
**Skills learned**: Rust middleware, rate limiting, security

---

### 19. Document API error responses

**Difficulty**: Easy

Create a reference document listing all error codes with HTTP status, example JSON, cause, and fix guidance.

**Files**: new `docs/api-errors.md`
**Depends on**: Task 15 (error constants)
**Skills learned**: Technical documentation, API design

---

### 20. Add validation tests for account settings

**Difficulty**: Medium

Account validation logic in `account_support.rs` has no unit tests. Write tests for valid configs, invalid URLs, missing fields, and edge cases.

**Files**: `crates/posthaste-server/src/api/account_support.rs` (add `#[cfg(test)]` module)
**Depends on**: nothing
**Skills learned**: Rust unit testing, validation logic

---

### 21. Document posthaste-config schema

**Difficulty**: Easy

Config uses TOML but the schema is undocumented. Create `docs/config-schema.md` with field types, defaults, constraints, and a complete example.

**Files**: new `docs/config-schema.md`
**Depends on**: nothing
**Skills learned**: Configuration documentation, TOML

---

### 22. Document database schema

**Difficulty**: Easy

SQLite schema is defined in code but not documented. Create `docs/database-schema.md` with tables, columns, keys, indexes, and a text-based ER diagram.

**Files**: new `docs/database-schema.md`
**Depends on**: nothing
**Skills learned**: Database documentation, data modeling

---

### 23. Extract SQL constants from posthaste-store

**Difficulty**: Easy

Various Rust files have hardcoded table/column name strings. Create `crates/posthaste-store/src/db/constants.rs` and update imports.

**Files**: new `crates/posthaste-store/src/db/constants.rs`, `crates/posthaste-store/src/db.rs`
**Depends on**: nothing
**Skills learned**: Rust modules, constants, maintainability

---

### 24. Add doc comments to public functions

**Difficulty**: Easy

Public functions in posthaste-server and posthaste-store lack `///` doc comments. Add them and verify with `cargo doc --open`.

**Files**: multiple files in `crates/posthaste-server/src/api/`
**Depends on**: nothing
**Skills learned**: Rust doc comments, `cargo doc`

---

## Project Infrastructure

### 25. Set up frontend testing framework (Vitest + RTL)

**Difficulty**: Medium

Zero frontend tests exist. Add `vitest`, `@testing-library/react`, and `@testing-library/user-event`. Create `vitest.config.ts`, a test script, and one example test for `formatRelativeTime`.

**Files**: `web/package.json`, new `web/vitest.config.ts`, new `web/src/utils/formatRelativeTime.test.ts`
**Depends on**: nothing
**Skills learned**: Test framework setup, npm tooling

---

### 26. Add pre-commit hook for linting

**Difficulty**: Medium

Add a hook that runs `npm run lint` in web/ and `cargo clippy` in crates/ before each commit.

**Files**: hook script, `README.md` update
**Depends on**: nothing
**Skills learned**: Git hooks, shell scripting, CI/CD

---

### 27. Document development setup in README

**Difficulty**: Easy

README lacks step-by-step setup instructions. Add prerequisites, clone-and-run steps, how to start both dev servers, env var examples, and troubleshooting.

**Files**: `README.md`
**Depends on**: nothing
**Skills learned**: Technical writing, developer onboarding

---

### 28. Create CONTRIBUTING.md

**Difficulty**: Easy

Add contribution guidelines: branch/commit conventions, where to find tasks (this file), code style, how to run tests, PR process.

**Files**: new `CONTRIBUTING.md`
**Depends on**: nothing
**Skills learned**: Open source practices, documentation

---

### 29. Add CI workflow (GitHub Actions)

**Difficulty**: Medium

No CI pipeline. Create `.github/workflows/test.yml` that runs frontend lint+test and backend `cargo test`+`cargo clippy` on every push and PR. Add status badge to README.

**Files**: new `.github/workflows/test.yml`, `README.md`
**Depends on**: Task 25 (frontend tests)
**Skills learned**: GitHub Actions, CI/CD pipelines

---

### 30. Create issue templates

**Difficulty**: Easy

Add `.github/ISSUE_TEMPLATE/bug_report.md` and `feature_request.md` with structured sections (description, repro steps, environment, screenshots).

**Files**: new `.github/ISSUE_TEMPLATE/bug_report.md`, new `.github/ISSUE_TEMPLATE/feature_request.md`
**Depends on**: nothing
**Skills learned**: Issue management, community guidelines

---

## Onboarding Plan

### Week 1 (high-impact quick wins)

| Task | Why first |
|------|-----------|
| 27 — Dev setup docs | Forces them to set up the project and document gaps |
| 25 — Testing framework | Unblocks all future frontend tests |
| 1 — Loading skeletons | Teaches React Query + Tailwind patterns |
| 2 — Accessibility labels | Quick win, immediate UX improvement |

### Week 2 (expanding skills)

| Task | Why next |
|------|----------|
| 3 — Error boundary | Advanced React pattern |
| 10 — formatBytes + tests | First test-writing exercise |
| 15 — Error code constants | Intro to Rust, minimal domain knowledge |
| 24 — Doc comments | Reading and understanding Rust code |

### Week 3+ (polish and infrastructure)

Pick from remaining tasks based on interest. Frontend-leaning devs: 4--14. Backend-curious devs: 16--23. Infra-minded: 26, 28--30.

### Milestones

**End of Week 1**: Can run both dev servers, made 1--2 merged contributions, understands component architecture.

**End of Week 2**: Comfortable with testing patterns, familiar with Rust module structure, understands API boundary.

**End of Month 1**: Can implement a full React component with error handling, can write basic Rust following project patterns, understands frontend-backend communication.
