// Explicit regression test for the IndexedDB version-skew bug that blocks the
// mail list on .26. Drives the REAL store classes (via Vite dynamic import) on
// a clean `posthaste-replica` DB from a blank page (so the app never opens it
// first), then cross-checks with the raw open sequence.
//
// Bug: undoHistoryStore opens `posthaste-replica` at DB_VERSION=2 (onupgradeneeded
// creates `undoHistory` ONLY — not `outbox`). When it opens first (MailClient ->
// useUndoRedo -> getUndoHistoryStore), the DB jumps to v2. Then openMailListView's
// rehydration calls outbox.all() -> outboxStore opens `posthaste-replica` at
// VERSION=1 -> a version DOWNGRADE -> onerror -> outbox.all() rejects -> swallowed
// by useRuntimeMailListView's .catch -> isLoading stays true -> mail list hangs.
import { readFileSync } from 'node:fs'

const PW_CORE =
  process.env.POSTHASTE_PLAYWRIGHT_CLI?.replace(/\/cli\.js$/, '/index.mjs') ??
  'playwright-core'
const { chromium } = await import(PW_CORE)

const URL = 'http://127.0.0.1:5173/blank-idb-probe'
const BLANK = `<!DOCTYPE html><html><head><script type="module">
window.__PROBE__ = (async () => {
  const DB = 'posthaste-replica'
  const log = []
  const step = (m) => log.push(m)
  const del = () => new Promise((res) => { const r = indexedDB.deleteDatabase(DB); r.onsuccess = r.onerror = r.onblocked = () => res(); })
  const open = (v, upgrade) => new Promise((resolve, reject) => { const r = indexedDB.open(DB, v); if (upgrade) r.onupgradeneeded = () => upgrade(r.result); r.onsuccess = () => resolve(r.result); r.onerror = () => reject(r.error); })
  try {
    step('start')
    await del(); step('deleted')
    const { IndexedDbUndoHistoryStore } = await import('/src/runtime/replica/undoHistoryStore.ts')
    const { IndexedDbOutboxStore } = await import('/src/runtime/replica/outboxStore.ts')
    step('imported stores')
    // Step 1 (mimics MailClient -> useUndoRedo -> getUndoHistoryStore -> load()):
    // opens at v2, onupgradeneeded creates 'undoHistory' ONLY.
    const undo = new IndexedDbUndoHistoryStore()
    await undo.load(); step('undo.loaded')
    // Inspect the DB the undo store left behind.
    let inspect
    try { const db = await open(2); inspect = { version: db.version, stores: [...db.objectStoreNames] }; db.close() }
    catch (e) { inspect = { error: e?.name ?? String(e) } }
    step('inspect=' + JSON.stringify(inspect))
    // Step 2 (mimics openMailListView -> outbox.all() -> openConnection at v1):
    let outboxAllError = null
    try { const outbox = new IndexedDbOutboxStore(); await outbox.all(); step('outbox.all NO error (unexpected)') }
    catch (e) { outboxAllError = e?.name ?? String(e); step('outbox.all errored=' + outboxAllError) }
    return { log, inspect, outboxAllError }
  } catch (e) { return { log, fatal: e?.name ?? String(e), msg: e?.message ?? '' } }
})()
</script></head><body>idb probe</body></html>`

const browser = await chromium.launch({ headless: true })
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } })
page.on('console', (m) => console.log('[browser]', m.text().slice(0, 180)))
page.on('pageerror', (e) => console.log('[PAGEERR]', e.message.slice(0, 250)))
await page.route(URL, (r) => r.fulfill({ contentType: 'text/html', body: BLANK }))
await page.goto(URL, { waitUntil: 'load', timeout: 20000 })
const probe = await page.evaluate(() => window.__PROBE__)
await browser.close()

console.log('=== REAL store classes: undo.load() then outbox.all() ===')
console.log('  steps:', probe.log.join(' | '))
if (probe.fatal) console.log('  FATAL:', probe.fatal, probe.msg)
console.log('  after undo.load() — DB inspect:', JSON.stringify(probe.inspect))
console.log('  outbox.all() error:', JSON.stringify(probe.outboxAllError))

const bugPresent = probe.outboxAllError !== null
console.log(
  bugPresent
    ? `\n>>> FAIL: outbox.all() rejected with ${probe.outboxAllError} after undo.load() — the version-skew bug is present (DB at v2 with no 'outbox' store).`
    : '\n>>> PASS: outbox.all() succeeded after undo.load() — the shared DB schema is coordinated (no version skew).',
)
process.exit(bugPresent ? 1 : 0)
