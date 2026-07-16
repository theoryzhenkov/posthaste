# apps/client — the integrated app

A lightweight mirror client over the domain core. The backend is the ONE
evaluator — it materializes windowed **surfaces** from the store's
`_effective` views into a versioned per-session **state document**; the
frontend is a dumb mirror (subscribe → apply patch → render) that sends
**commands** (typed mail intents) back. Optimism lives only in the backend's
overlay and is invisible here.

Layout:

- `models/` — Rust crate, the protocol's single source of truth (document,
  surfaces, patches, commands). TS types are GENERATED from it (ts-rs) into
  `frontend/src/gen/`. One codegen pipeline, ever.
- `backend/` — Rust binary+lib: sessions, the surface materializer, the
  dirty→coalesce→diff recomputer, the SSE stream, the command endpoint
  feeding the outbox.
- `frontend/` — bun + vite + React + TypeScript and deliberately almost
  nothing else. The mirror store is hand-rolled (~100 lines around
  `useSyncExternalStore`); no react-query, no state framework. Every
  dependency here is something a hacking user must understand.

Conventions: comments and docs in this app describe the LIVE state of the
app — decision history and rationale live in `docs/eph` (design record:
`docs/eph/RFC-L2-mirror-client.md`), not in code comments.

## Decided

- **Transport is localhost HTTP + SSE** — the same surface for UI, CLI, MCP,
  and user scripts; the client runs in a bare browser tab; remote attachment
  stays possible. SSE over WebSocket: one-directional stream + POST commands;
  our recovery model needs no bidirectional framing.
- **Tauri v2 arrives later, as a thin shell** (window, tray/background,
  notifications, single-instance, the existing updater/signing machinery) —
  never as a transport. The first slices need no shell at all.
- **Recovery == connect**: snapshot + seq-numbered patches; any gap or doubt
  → refetch the (screen-sized) document.
- **`ReplaceSurface`-first patching**: the dumbest correct patch ships first;
  row-level diffs are a measured optimization inside the same protocol, not a
  protocol assumption.
- **Dependency allowlist**: `models` = serde + ts-rs + domain-model;
  `backend` = domain core + models + HTTP stack. Nothing from the
  runtime/link/replica seam (posthaste-runtime, link crates, contract-core,
  replica-*) may enter `apps/client` — the boundary is stated in each
  Cargo.toml and should graduate to a CI check (wasm-frontier-style) when the
  first real code lands.
- Workspace members but NOT default-members: the default build (and nightly)
  stays the split-model app until the cutover.

## Deliberately undecided (design before code)

- The document/session granularity: one document per session vs per-surface
  streams under one seq domain.
- Patch representation (typed enum vs JSON-patch-shaped) and the exact seq /
  ack semantics.
- Surface vocabulary + windowing commands (what `extend_window` looks like,
  how sort/scope are named — reusing the mail-query grammar vs a typed enum).
- How the materializer reuses the existing mail-query/`build_snapshot` code
  without inheriting view-registry assumptions.
- Recomputer tick/coalescing policy and its perf gate numbers.
- Backend process model for dev vs shipped app (standalone binary now;
  embedded vs sidecar under Tauri is a late, severable decision).

First implementation slice (once shapes are decided): one surface (inbox
list) end-to-end — materialize → stream → mirror → render, an archive command
through the outbox, and the live convergence test consuming the document via
a testkit session.
