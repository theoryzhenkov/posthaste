// P2 runtime validation: confirm the WorkerStorePort keeps the UI thread
// responsive during a re-sync–style burst of `message.updated` notifications.
//
// The freeze root cause was per-event WASM ingest + projection on the JS main
// thread during a full re-sync (thousands of events). Stage 0 coalesces the
// burst into one ingest+projection per frame; P2 offloads that (still-heavy)
// WASM work to a Web Worker. This probe drives the REAL adapter + REAL WASM
// (via Vite, in a real browser) with a fake base that emits a burst, while a
// requestAnimationFrame probe measures main-thread frame gaps:
//   - worker path: the flush's WASM work runs on the worker → main thread
//     stays free → rAF gaps stay small.
//   - in-process path (baseline): the flush's WASM runs on the main thread →
//     one long rAF frame → a large gap.
//
// Run: POSTHASTE_PLAYWRIGHT_CLI=<nix devShell path> bun apps/web/e2e/worker-burst-probe.mjs
import { readFileSync } from 'node:fs'

const PW_CORE =
  process.env.POSTHASTE_PLAYWRIGHT_CLI?.replace(/\/cli\.js$/, '/index.mjs') ??
  'playwright-core'
const { chromium } = await import(PW_CORE)

const URL = 'http://127.0.0.1:5173/worker-burst-probe'
const BLANK = `<!DOCTYPE html><html><head><script type="module">
window.__PROBE__ = (async () => {
  const log = []
  const step = (m) => log.push(m)
  try {
    step('start')
    const { createEntityStoreAdapter } = await import('/src/runtime/replica/entityStoreAdapter.ts')
    const { InProcessStorePort } = await import('/src/runtime/replica/storePort.ts')
    const { createWorkerStorePort } = await import('/src/runtime/replica/workerStorePort.ts')
    const { loadEntityStoreHandleFactory } = await import('/src/runtime/replica/handle.ts')
    const { MemoryOutboxStore } = await import('/src/runtime/replica/outboxStore.ts')
    // Import the app's singleton QueryClient via a Vite-served path (a bare
    // '@tanstack/react-query' import wouldn't resolve from an inline script).
    const { queryClient: singletonQC } = await import('/src/app/queryClient.ts')
    const { queryKeys } = await import('/src/queryKeys.ts')
    step('imports ok')

    const SEED_ROWS = 50
    const BURST = ${process.env.WORKER_BURST ?? 20000}

    function snapshot() {
      const rows = Array.from({ length: SEED_ROWS }, (_, i) => ({
        rowKey: 's:m' + i,
        resourceRef: 'message:s:m' + i,
        projection: {
          id: 'm' + i, sourceId: 's', receivedAt: '2026-04-29T10:00:00Z',
          keywords: [], mailboxIds: ['inbox'], isRead: false, isFlagged: false,
          subject: 'm' + i,
        },
        orderKey: 'm' + i,
      }))
      return {
        viewId: 'v1', descriptor: { family: 'mailList', payload: {} },
        revision: 1, lifecycle: 'ready', readWatermark: null, coverage: { ranges: [] },
        data: {
          scope: null, projectionKind: 'message', sort: null, windowRequest: null,
          rows, continuation: { beforeCursor: null, afterCursor: null, hasBefore: false, hasAfter: false },
          readWatermark: null, coverage: { ranges: [] }, knownTotalCount: SEED_ROWS,
          pendingMutations: [], anchor: null,
        },
        pendingMutations: [], error: null,
      }
    }

    function makeBase() {
      let sink = null
      const base = {
        openRuntimeSessionMessageListView: async () => ({ viewId: 'v1', snapshot: snapshot() }),
        extendRuntimeSessionView: async () => ({ viewId: 'v1', snapshot: snapshot() }),
        closeRuntimeSessionView: async () => ({ ok: true }),
        subscribeRuntimeFrames: (_req, handlers) => { sink = handlers; return () => { sink = null } },
        runRuntimeMutation: async (req) => ({ runtimeMutationId: 'r', clientMutationId: req.clientMutationId, name: req.name, state: 'accepted', error: null }),
      }
      return { base, push: (f) => sink?.onFrame(f) }
    }

    const viewRequest = {
      sessionId: 'sess',
      view: {
        scope: { kind: 'source-mailbox', sourceId: 's', mailboxId: 'inbox' },
        limit: 50, sort: 'date', sortDir: 'desc', operation: { name: 'probe' },
      },
    }

    function msgUpdated(i) {
      return {
        type: 'notification', sessionSeq: 100, kind: 'message.updated',
        payload: {
          seq: 1, accountId: 's', topic: 'message.updated', occurredAt: 'now',
          payload: {
            messageId: 'm0',
            projection: {
              id: 'm0', sourceId: 's', receivedAt: '2026-04-29T10:00:00Z',
              keywords: [], mailboxIds: ['inbox'], isRead: false, isFlagged: false,
              subject: 'burst-' + i,
            },
            countDeltas: [],
          },
        },
      }
    }

    function startRafProbe() {
      const times = []
      let id = requestAnimationFrame(function loop(t) { times.push(t); id = requestAnimationFrame(loop) })
      return { times, stop: () => cancelAnimationFrame(id) }
    }

    async function runPath(label, storeDeps) {
      const harness = makeBase()
      const outbox = new MemoryOutboxStore()
      const queryClient = singletonQC
      queryClient.setQueryData(queryKeys.mailboxes('s'), [{ id: 'inbox', name: 'Inbox', role: 'inbox', unreadEmails: 0, totalEmails: SEED_ROWS }])
      const adapter = createEntityStoreAdapter({ base: harness.base, outbox, queryClient, now: () => 1, ...storeDeps })
      const frames = []
      adapter.subscribeRuntimeFrames({ sessionId: 'sess' }, { onFrame: (f) => frames.push(f) })
      await adapter.openRuntimeSessionMessageListView(viewRequest)
      step(label + ': view opened')

      const probe = startRafProbe()
      const lastSubject = 'burst-' + (BURST - 1)
      // Push the whole burst synchronously (as a re-sync would deliver it).
      const t0 = performance.now()
      for (let i = 0; i < BURST; i++) harness.push(msgUpdated(i))
      // Wait until the coalesced flush projects the LAST update (drain signal).
      let drained = false
      const drainStart = performance.now()
      await new Promise((resolve) => {
        const deadline = performance.now() + 45000
        function check() {
          const last = [...frames].reverse().find((f) => f.type === 'viewReplace' || f.type === 'viewSnapshot')
          const m0 = last?.snapshot?.data?.rows?.find((r) => r.projection?.id === 'm0')
          if (m0?.projection?.subject === lastSubject) { drained = true; return resolve() }
          if (performance.now() > deadline) return resolve()
          setTimeout(check, 5)
        }
        setTimeout(check, 5)
      })
      const drainMs = performance.now() - drainStart
      // Let a couple more rAFs tick so the probe captures the post-flush frame.
      await new Promise((r) => setTimeout(r, 120))
      probe.stop()
      const times = probe.times
      let maxGap = 0
      for (let i = 1; i < times.length; i++) maxGap = Math.max(maxGap, times[i] - times[i - 1])
      step(label + ': drainMs=' + drainMs.toFixed(0) + ' rafFrames=' + times.length + ' maxGap=' + maxGap.toFixed(0) + ' drained=' + drained)
      return { label, drainMs, rafFrames: times.length, maxGap, drained }
    }

    // Worker path first (spawns the worker; WASM loads in the worker).
    const workerResult = await runPath('worker', { makeStore: createWorkerStorePort })
    // In-process baseline (WASM loads on the main thread).
    const makeHandle = await loadEntityStoreHandleFactory()
    const inprocResult = await runPath('in-process', { makeHandle: () => makeHandle() })

    return { log, workerResult, inprocResult, burst: BURST }
  } catch (e) {
    return { log, fatal: e?.name ?? String(e), msg: e?.message ?? '', stack: e?.stack ?? '' }
  }
})()
</script></head><body>worker burst probe</body></html>`

const browser = await chromium.launch({ headless: true })
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } })
page.on('console', (m) => console.log('[browser]', m.text().slice(0, 200)))
page.on('pageerror', (e) => console.log('[PAGEERR]', e.message.slice(0, 300)))
await page.route(URL, (r) =>
  r.fulfill({ contentType: 'text/html', body: BLANK }),
)
await page.goto(URL, { waitUntil: 'load', timeout: 30000 })
const probe = await page.evaluate(() => window.__PROBE__)
await browser.close()

console.log('=== WorkerStorePort burst-responsiveness probe ===')
console.log('  steps:', probe.log.join(' | '))
if (probe.fatal) console.log('  FATAL:', probe.fatal, probe.msg)

const w = probe.workerResult
const b = probe.inprocResult
console.log('\n  burst size:', probe.burst)
console.log('  worker     :', JSON.stringify(w))
console.log('  in-process :', JSON.stringify(b))

// The worker path must (1) drain the burst, and (2) keep the main thread
// responsive — max rAF gap well under one jank frame (~50ms). The in-process
// baseline is reported for comparison (it janks under a large burst); the worker
// is not required to beat it on drain *time* (postMessage round-trips make the
// worker drain slower) — only to keep the UI frame budget intact.
const JANK_MS = 50
const ok = !probe.fatal && w?.drained === true && w?.maxGap < JANK_MS

console.log(
  ok
    ? '\n>>> PASS: worker drained the burst with max rAF gap ' +
        w?.maxGap?.toFixed(0) +
        'ms (< ' +
        JANK_MS +
        'ms); UI stayed responsive (in-process maxGap=' +
        b?.maxGap?.toFixed(0) +
        'ms).'
    : '\n>>> FAIL: worker did not keep the UI responsive (drained=' +
        w?.drained +
        ', maxGap=' +
        w?.maxGap +
        'ms).',
)
process.exit(ok ? 0 : 1)
