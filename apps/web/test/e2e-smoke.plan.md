# Client resilience — Playwright smoke plan (D119)

**Status: PLAN ONLY — deliberate scope cut for M48.**

This sandbox cannot run a real browser (no Playwright dependency is installed,
and adding one is out of scope for M48 — zero new deps). The five smoke
scenarios the RFC calls for are specified here so wiring Playwright later is
mechanical: each scenario lists the exact driver steps and the observable
assertions. The deterministic unit-level coverage of the SAME failure modes
already lives in `test/harness/scenarios/*` (fake transport / fake worker /
virtual clock); this smoke layer is the thin end-to-end confirmation over a real
browser + real worker + real WASM that the wiring holds in aggregate.

## Prerequisites (when wired)

- Add `@playwright/test` as a devDependency; `playwright install chromium`.
- A `playwright.config.ts` with `webServer` running `vite preview` (or `vite dev`)
  against a seeded loopback backend fixture (the same fixture the `e2e/` probes use).
- A stable `data-testid` surface on: the sidebar unread counter, the mail-list
  rows, the compose/trash actions, and the degraded-state indicator (M45).
- A backend control channel to reap/restart the link and to kill the worker
  (or drive worker-kill from the page via an exposed test hook).

## Scenario 1 — open (baseline liveness)

- **Steps**: launch app → wait for the entity-store adapter to install → open the
  Inbox view.
- **Assert**: the mail list renders the seeded rows; the sidebar unread counter
  matches the seeded count; no error toast; connection health is `healthy`.

## Scenario 2 — trash → counter (S2 regression)

- **Steps**: select a message in the Inbox → trash it.
- **Assert**: the row disappears from the list immediately (optimistic fold);
  within the round-trip the sidebar unread/total counter decrements (authority
  count over the live stream, D113). The row-gone-but-badge-stale symptom (S2)
  must NOT appear.

## Scenario 3 — sleep/wake resume (F1 / M40)

- **Steps**: open Inbox → simulate a >5min idle so the link is idle-reaped (or
  reap it via the backend control channel) → trigger a wake (foreground the tab)
  → cause an authoritative update on the server (e.g. deliver a new message).
- **Assert**: the stream re-prepares WITHOUT a page reload (the 404-on-reopen no
  longer permanently halts, M40); the new message appears in the list and the
  counter updates. Cross-check the TS-seam unit proof:
  `scenarios/streamSeverReopen.test.ts`.

## Scenario 4 — worker-kill recovery (F3 / M31 → M42)

- **Steps**: open Inbox → kill the store worker (page test hook / devtools) mid-
  session → cause an authoritative update.
- **Assert (M31 baseline)**: the app does not hang; the watchdog respawns the
  worker. **Assert (once M42 lands)**: the views re-populate, the pending set
  replays, and counts recover — no reload. Until M42, assert only liveness +
  respawn (mirrors the port-level unit proof in
  `scenarios/workerWedgeRestart.test.ts`).

## Scenario 5 — reconnect (transient link loss)

- **Steps**: open Inbox → drop the stream transiently (no status / network blip)
  via the control channel → let it come back.
- **Assert**: the client reconnects on its own (engine-owned backoff), resumes
  from the resume cursor, and the list/counter stay consistent after recovery;
  a brief degraded indicator (M45) may show while recovering, then clears to
  `healthy`.

## Mapping to the deterministic harness

| Smoke scenario | Failure mode | Deterministic unit proof                                                           |
| -------------- | ------------ | ---------------------------------------------------------------------------------- |
| 3 sleep/wake   | F1 / M40     | `scenarios/streamSeverReopen.test.ts`                                              |
| 4 worker-kill  | F3 / M31,M42 | `scenarios/workerWedgeRestart.test.ts`                                             |
| (unload)       | W3 / N18     | `scenarios/unloadFlush.test.ts`                                                    |
| 5 reconnect    | transient    | `nearEndEngine.test.ts` (engine backoff) + gap: `scenarios/streamGapFrame.test.ts` |
| 1–2 open/trash | S1 / S2      | `entityStoreAdapter.test.ts` + the harness open/emit path                          |
