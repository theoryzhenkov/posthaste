---
scope: L2
summary: "Split the one boolean that answered two unrelated questions in the client's app root. Liveness — the stream subscription that invalidates a window's mirror — belongs in EVERY window, and is now inseparable from the mirror itself: the provider that creates a window's QueryClient is the provider that subscribes it, and no bare client is exported. Ownership of the process-wide OS surfaces (Dock badge, new-mail banners) stops being inferred from window identity and becomes an explicit leased claim that survives its holder closing."
modified: 2026-07-31
reviewed: 2026-07-31
lifecycle: ephemeral
type: DESIGN
state: implemented
depends:
  - path: apps/client/frontend/src/data/queries/mirror.tsx
    note: "the live mirror — client construction and stream subscription in one provider"
  - path: apps/client/frontend/src/lib/platform/sharedOsSurfaces.ts
    note: "the leased ownership claim"
  - path: apps/client/frontend/src/app/App.tsx
  - path: apps/client/frontend/src/data/transport/stream.ts
    section: "the ONE invalidation policy this hangs off"
dependents: []
---

# DESIGN-L2-window-liveness — a live mirror per window, one owner for the OS

> **Status: DESIGN — IMPLEMENTED (2026-07-31).** Three migration steps landed as
> one commit each on top of `main`; each was gated independently. The ownership
> claim is unit-tested (14 tests, two mutations checked). Nothing about React
> composition or real multi-window behaviour is tested — the frontend suite has
> no DOM — and nobody ran the desktop app, so every claim below about what a
> second window does at runtime is an inference from the code.

## 1. Problem

A user reported sync progress freezing: "Fetching mailboxes" on JMAP,
"Connecting account" on IMAP, stuck forever, with a reload showing the correct
current state.

`App.tsx` gated four bridges behind `!isStandaloneSurface`, where
`isStandaloneSurface` is `isTauriRuntime() && routeState.kind !== 'none' &&
!isMainDesktopWindow()` — a surface (settings, compose, message, attachment)
opened in its own desktop window. Two of the four were `StreamInvalidationBridge`
and `ConnectionBanner`, and the comment stated the intent plainly: "Liveness
rides the facade's event stream in the MAIN window; a standalone surface window
keeps its queries mount-fetched only."

Mount-fetched only is not a reduced service level here, it is a permanent
freeze. `queries/queryClient.ts` set `staleTime: Infinity`,
`refetchOnWindowFocus: false` and `refetchOnReconnect: false`, on a deliberate
and correct policy: *liveness is the stream's job, not the browser's*. Remove
the stream and nothing is left. A surface window fetched every query once at
mount and never again for the life of the window. The reload that "fixed" it was
the remount.

### 1.1 One boolean, two questions

The gate conflated two things that only look alike:

| bridge | what it is | who should run it |
|---|---|---|
| `StreamInvalidationBridge` | keeps THIS window's cache from going stale | every window |
| `ConnectionBanner` | reports THIS window's stream status | every window |
| `NewMailNotificationsBridge` | posts OS banners | exactly one window |
| `DockBadge` | drives the app-wide icon counter | exactly one window |

The comment's worry — "duplicate liveness" — does not apply to the first pair.
Each webview is its own JS realm with its own `QueryClient`; a second window
invalidating can only refresh its own mirror. There is no shared cache to
duplicate against. The second pair is a genuine singleton: the Dock badge is one
counter on one app icon, and an arrival bannered by two windows is two banners.

And `isMainDesktopWindow()` is the wrong test even for the second pair. It
answers "which window is this", not "may this window drive the shared
surfaces", and the two stop agreeing the moment a window closes: close the main
window while a settings window stays open and NOBODY owns the badge or the
banners for the rest of the session. In the browser build the probe cannot be
asked at all (`isStandaloneSurface` requires `isTauriRuntime()`), so every
`window.open` surface popup ran both effects.

## 2. Design

### 2.1 Liveness in every window

The first pair comes out of the gate. Nothing else about the chain changes: the
supervisor writes progress under matching generation/cycle tokens and publishes,
`EventBus::publish` bumps the generation once per non-empty batch, and
`GET /events` hands each connection its own `broadcast::Receiver`, so N windows
means N SSE connections all receiving every event and reading the same global
generation. Multi-window liveness needed no backend change and got none.

### 2.2 The mirror carries its own liveness

Ungating is a fix, not a design. What let the defect exist is that getting data
into a window took two independent acts: import the module-level `queryClient`,
and *separately remember* to mount the bridge. Skipping the second is silent at
review time and permanent at runtime.

So the two acts became one. `data/queries/mirror.tsx` replaces
`queries/queryClient.ts` and exports **only** `MirrorProvider`, which creates
this window's `QueryClient` and renders the subscription as its first child —
inside its own `QueryClientProvider`, so the subscription invalidates the same
client every consumer below it reads. No client and no factory are exported. A
mirror that nothing keeps live is not a discouraged pattern; it has no
expression.

The client also stops being module-level state. It was already per-window in
effect, but as a module singleton that was an accident of how the bundle loads
rather than something stated; `useState(createMirror)` states it.

### 2.3 Ownership as a claim, not an inference

`lib/platform/sharedOsSurfaces.ts` replaces the identity probe with one named
concept: the **shared OS surfaces**, and a leased claim on them. Windows race
for a single slot in shared storage holding `{ holder, expiresAt }`. The holder
renews every 2s against a 5s lease; a lease nobody renews expires and the next
polling window takes it. First claimant keeps winning, so the window that booted
first — the main one — holds it in practice, and a surface window inherits it
only once the holder is gone.

Two properties matter and both are tested:

- **Recovery.** A holder that stops renewing — closed, crashed, frozen — loses
  the claim, and a peer has it within roughly lease + renew. Nothing depends on
  an unload handler running, which is exactly what a closing window cannot
  promise.
- **A corrupt record reads as vacant.** Absent, unparseable, misshapen and
  expired are one case. A bad write can never lock every window out of the OS
  surfaces permanently.

It runs on the R8 seams (`lib/ambient/storage`, `time`, `random`), so the whole
thing is exercised through a fake shared storage and a fake clock with no DOM
in sight.

## 3. Migration (one commit per step, each independently shippable)

| step | commit | contents |
|---|---|---|
| 1 | `fix(client): subscribe every window to the stream, not just the main one` | `StreamInvalidationBridge` + `ConnectionBanner` out of the gate; the stale comments rewritten. **The freeze dies here.** |
| 2 | `refactor(client): make the mirror inseparable from its stream subscription` | `mirror.tsx` replaces `queryClient.ts`; the bare client stops being exported; `ThemeProvider` moves to `useQueryClient()`. |
| 3 | `feat(client): claim ownership of the shared OS surfaces instead of inferring it` | `sharedOsSurfaces.ts` + its 14 tests; the two OS bridges gate on the claim. |

## 4. What the diagnosis got wrong

Little, and nothing load-bearing — the read was accurate. Three corrections:

- **The direct-importer count.** The brief named `ThemeProvider.tsx:20` plus the
  `data/index.ts` re-export, and asked for the count to be verified rather than
  trusted. It holds: `App.tsx` was the only other one, and every remaining
  `queryClient` identifier in `src/` is either a local from `useQueryClient()`
  or a function parameter. The brief's worry that helpers like
  `ensureAppSettings`/`runCommand` would need rerouting was unfounded — they
  take the client as an argument, and their call sites were already on context.
- **`ConnectionBanner` is not part of the freeze mechanism.** It was grouped
  with liveness, correctly, but it subscribes to nothing that invalidates. Its
  own defect was quieter: a surface window rendered no banner at all, so a
  window whose stream was down said nothing while its queries sat frozen.
- **The web build was never affected by the freeze, and was always affected by
  the double-notify.** `isStandaloneSurface` requires `isTauriRuntime()`, so in
  the browser every popup surface window already had liveness — and already ran
  the arrival gate and the badge driver. Step 3 fixes a standing web defect the
  brief did not mention, in the opposite direction from the desktop one.

## 5. Honest limits

- **No DOM, so no composition test.** `bun test` here has no jsdom and no test
  touches `document`; a fake-DOM harness was deliberately not introduced. That
  `MirrorProvider` mounts, that its subscription runs, that
  `useOwnsSharedOsSurfaces` flips after its first poll and releases on unmount —
  none of it is covered. Only the claim logic underneath the hook is.
- **Cross-window storage is an inference.** The claim assumes every window of
  the app reads one `localStorage`. True by construction for browser popups; for
  Tauri webviews it follows from their sharing an origin and a data store, and
  is not exercised by any test here.
- **The lease is not mutual exclusion.** Web storage has no compare-and-swap.
  Two windows finding the same lease expired in the same instant can both write;
  the read-back narrows that race without closing it. Losing it costs one
  duplicated banner or one redundant badge push, and the next renewal converges.
- **Handover has a gap.** For up to lease + renew (~7s) after a holder dies,
  nobody owns the surfaces: banners in that window are dropped and the badge is
  stale. Better than the hole it replaces, which had no recovery at all.
- **5s / 2s are chosen, not measured.** They trade handover latency against how
  much timer throttling a backgrounded holder can absorb before a peer preempts
  it. A flap is churn, not corruption — the effects are edge-triggered — but the
  numbers have not been tuned against a real desktop session.
- **`useDesktopUpdates` still gates on `isMainDesktopWindow()`.** Same class of
  hole, left alone: it is a once-per-launch check rather than a continuous
  effect, and folding it in means first deciding what an update prompt in a
  compose window should do.
- **`DesignThemeProvider` now requires a `MirrorProvider` ancestor** (it reads
  the mirror from context). Noted on the prop; only `App` mounts it today.

## 6. Is the freeze fixed, or structurally impossible?

Structurally impossible **for the mechanism identified**:

1. The gate that removed the subscription is gone; every window mounts the same
   provider.
2. There is no way to obtain a `QueryClient` without the subscription — no
   exported client, no exported factory. A future window that renders a query
   must go through `MirrorProvider`, which is also what subscribes it.
3. The query defaults were not weakened. `refetchOnWindowFocus` and
   `refetchOnReconnect` stay off, so nothing masks a missing subscription into a
   slow-but-eventually-correct UI. If liveness ever breaks again it breaks
   loudly, in the same recognisable way.

The residual uncertainty is diagnostic, not structural. Nobody rendered the bug:
"a surface window never subscribed, so its progress query never refetched"
remains an inference from the gate, the `staleTime: Infinity` policy, and
"reload fixes it". If the freeze survives this change, the subscription is being
made but not delivering, and the next suspects are the second window's SSE
connection and the generation bump — neither of which this touches, and both of
which the backend read says are fine.

## 7. Code anchors

- The live mirror: `apps/client/frontend/src/data/queries/mirror.tsx`
- The invalidation policy it subscribes: `apps/client/frontend/src/data/transport/stream.ts`
- Ownership claim + guarantees: `apps/client/frontend/src/lib/platform/sharedOsSurfaces.ts`
- Its tests: `apps/client/frontend/src/lib/platform/sharedOsSurfaces.test.ts`
- Composition root: `apps/client/frontend/src/app/App.tsx`
- The SSE fan-out (unchanged): `apps/client/backend/src/api/events.rs`
- Prior evidence on the recurring non-liveness class: `docs/eph/AUDIT-L2-client-liveness.md`
