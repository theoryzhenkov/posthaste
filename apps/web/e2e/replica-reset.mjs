// Regression test for the "Repair does nothing" fix: `resetReplicaDatabase`
// must clear the `posthaste-replica` IndexedDB (outbox + undoHistory) EVEN while
// a store holds an open connection — `deleteDatabase` otherwise blocks
// indefinitely. Drives the REAL store classes (via Vite dynamic import) in a
// real browser, mirroring e2e/idb-version-conflict.mjs.
//
// Before the fix the only repair rebuilt server-side mail.sqlite and never
// touched this replica, so a wedged replica (the real cause of "views stuck
// loading forever") survived. This asserts the replica is actually cleared.
import { readFileSync } from 'node:fs'

const PW_CORE =
  process.env.POSTHASTE_PLAYWRIGHT_CLI?.replace(/\/cli\.js$/, '/index.mjs') ??
  'playwright-core'
const { chromium } = await import(PW_CORE)

const URL = 'http://127.0.0.1:5173/blank-replica-reset'
const BLANK = `<!DOCTYPE html><html><head><script type="module">
window.__PROBE__ = (async () => {
  const DB = 'posthaste-replica'
  const log = []
  const step = (m) => log.push(m)
  const del = () => new Promise((res) => { const r = indexedDB.deleteDatabase(DB); r.onsuccess = r.onerror = r.onblocked = () => res(); })
  try {
    step('start')
    await del(); step('deleted')
    const { IndexedDbOutboxStore } = await import('/src/runtime/replica/outboxStore.ts')
    const { resetReplicaDatabase } = await import('/src/runtime/replica/replicaDatabase.ts')
    step('imported')
    // Enqueue a record through the real store — this opens AND caches an open
    // connection (the condition that blocks deleteDatabase if untracked).
    const outbox = new IndexedDbOutboxStore()
    await outbox.put({ clientMutationId: 'c1', messageId: 'm1', assertion: { kind: 'setKeywords', add: ['flagged'], remove: [] }, runtimeMutationId: null, acceptedAt: 1 })
    const before = (await outbox.all()).length; step('before=' + before)
    // Reset with the connection still open. Must resolve quickly (not hang on a
    // block) — guard with a timeout so a regression shows as 'reset-timeout'.
    const t0 = Date.now()
    const timeout = new Promise((_, rej) => setTimeout(() => rej(new Error('reset-timeout')), 8000))
    await Promise.race([resetReplicaDatabase(), timeout])
    step('reset-ms=' + (Date.now() - t0))
    // A fresh store opens a brand-new (empty) DB.
    const after = (await new IndexedDbOutboxStore().all()).length; step('after=' + after)
    return { log, before, after }
  } catch (e) { return { log, fatal: e?.name ?? String(e), msg: e?.message ?? '' } }
})()
</script></head><body>replica reset probe</body></html>`

const browser = await chromium.launch({ headless: true })
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } })
page.on('console', (m) => console.log('[browser]', m.text().slice(0, 180)))
page.on('pageerror', (e) => console.log('[PAGEERR]', e.message.slice(0, 250)))
await page.route(URL, (r) =>
  r.fulfill({ contentType: 'text/html', body: BLANK }),
)
await page.goto(URL, { waitUntil: 'load', timeout: 20000 })
const probe = await page.evaluate(() => window.__PROBE__)
await browser.close()

console.log(
  '=== REAL store classes: put → resetReplicaDatabase (connection open) ===',
)
console.log('  steps:', probe.log.join(' | '))
if (probe.fatal) console.log('  FATAL:', probe.fatal, probe.msg)

const ok = !probe.fatal && probe.before === 1 && probe.after === 0
console.log(
  ok
    ? '\n>>> PASS: the held-open replica was reset and re-opened empty (before=1, after=0).'
    : `\n>>> FAIL: reset did not clear the replica (before=${probe?.before}, after=${probe?.after}${probe.fatal ? ', fatal=' + probe.fatal : ''}).`,
)
process.exit(ok ? 0 : 1)
