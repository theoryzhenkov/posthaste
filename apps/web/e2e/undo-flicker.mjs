// Playwright repro for the undo/redo "flash" (issue 1): undoing an archive
// briefly flashes a stale state before the view settles.
//
// Drives the REAL dev stack (just dev web) on .23. Archives a message, then
// undoes it via the toast Undo button, capturing the rendered row-set per
// paint (rAF) + every DOM mutation (MutationObserver). A flash = the restored
// row reappearing then disappearing (or any stale snapshot regressing) after
// the undo.
import { readFileSync } from 'node:fs'

const PW_CORE =
  process.env.POSTHASTE_PLAYWRIGHT_CLI?.replace(/\/cli\.js$/, '/index.mjs') ??
  'playwright-core'
const { chromium } = await import(PW_CORE)

const URL = 'http://127.0.0.1:5173'
const STATE_ROOT = process.env.POSTHASTE_STATE_ROOT || 'var/dev/posthaste/state'
const daemon = JSON.parse(readFileSync(`${STATE_ROOT}/daemon.json`, 'utf8'))
const ROWSEL = '.ph-scroll:has([data-message-list-empty]) > div > button'

const browser = await chromium.launch({ headless: true })
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } })
const errs = []
page.on('console', (m) => m.type() === 'error' && errs.push(m.text().slice(0, 140)))

await page.addInitScript(
  ([t, p]) => {
    window.__POSTHASTE_TOKEN__ = t
    window.__POSTHASTE_PORT__ = p
    window.__POSTHASTE_RUNTIME_MODE__ = 'loopback'
  },
  [daemon.token, daemon.port],
)
await page.goto(URL, { waitUntil: 'load', timeout: 30000 })
await page.waitForSelector(ROWSEL, { timeout: 20000 })
await page.waitForTimeout(1500)
await page.getByText('Inbox', { exact: true }).first().click()
await page.waitForTimeout(1500)

// Capture: rAF (painted states, deduped) + MutationObserver (childList add/remove).
await page.evaluate(() => {
  window.__FLASH__ = []
  let last = null
  function snap(tag) {
    const sp = document.querySelector('[data-message-list-empty]')
    const c = sp ? sp.parentElement : null
    const r = c ? [...c.querySelectorAll(':scope > div > button')].map((b) => b.textContent.replace(/\s+/g, ' ').trim().slice(0, 20)) : []
    const key = r.join('|')
    if (tag || key !== last) { window.__FLASH__.push({ t: performance.now().toFixed(0), tag: tag || null, n: r.length, rows: r }); last = key }
  }
  window.__SNAP__ = snap
  window.__MUT__ = []
  const mo = new MutationObserver((muts) => {
    for (const m of muts)
      if (m.target.closest?.('.ph-scroll')) window.__MUT__.push({ t: performance.now().toFixed(0), added: m.addedNodes.length, removed: m.removedNodes.length })
  })
  const c0 = document.querySelector('[data-message-list-empty]')?.parentElement
  if (c0) mo.observe(c0, { childList: true })
  ;(function loop() { snap(null); requestAnimationFrame(loop) })()
})

const before = await page.evaluate(() => window.__FLASH__.slice(-1)[0])
const archivedFp = before.rows[0]
console.log('pre-archive rows:', JSON.stringify(before.rows))

// Archive the first message.
await page.locator(ROWSEL).first().click()
await page.waitForSelector('button[aria-label="Archive"]', { state: 'visible', timeout: 5000 })
await page.locator('button[aria-label="Archive"]').click()
await page.evaluate(() => window.__SNAP__('archive-pressed'))
await page.waitForTimeout(1200) // let the archive settle + the toast appear
const afterArchive = await page.evaluate(() => window.__FLASH__.slice(-1)[0])
console.log('post-archive rows:', JSON.stringify(afterArchive.rows))

// Undo via the toast Undo button (fall back to Ctrl+Z).
const undoBtn = page.locator('[data-sonner-toast] button', { hasText: 'Undo' })
let undid = false
if (await undoBtn.count().catch(() => 0)) {
  await undoBtn.first().click()
  undid = true
} else {
  // keyboard fallback (Ctrl+Z)
  await page.keyboard.press('Control+z')
  undid = true
}
await page.evaluate((u) => window.__SNAP__(u ? 'undo-clicked' : 'undo-failed'), undid)
await page.waitForTimeout(1500)

// Redo via keyboard (Ctrl+Shift+Z) to re-apply the archive.
await page.keyboard.press('Control+Shift+z')
await page.evaluate(() => window.__SNAP__('redo-clicked'))
await page.waitForTimeout(2500)

const log = await page.evaluate(() => ({ flash: window.__FLASH__, mut: window.__MUT__ }))
console.log('--- rAF frame log ---')
for (const f of log.flash) console.log(`  t=${f.t.padStart(8)} ${(f.tag ?? '').padEnd(16)} | ${f.n}r: ${f.rows.join(' || ')}`)
console.log('--- MutationObserver (add/remove events around undo) ---')
const undoIdx = log.mut.findIndex((m) => true)
console.log(`  total events: ${log.mut.length}`)
console.log('  ' + log.mut.map((m) => `[${m.t} +${m.added}/-${m.removed}]`).join(' '))

// Analysis: track the archived fingerprint's presence across the whole
// archive -> undo -> redo trajectory. Expected transitions: 3
// (archive removes, undo restores, redo removes). Extra toggling = flash.
const presence = log.flash.map((f) => f.rows.includes(archivedFp))
let transitions = 0
for (let i = 1; i < presence.length; i++) if (presence[i] !== presence[i - 1]) transitions++
const restoredAfterUndo = presence.slice(log.flash.findIndex((f) => f.tag === 'undo-clicked') + 1).some(Boolean)
const absentAfterRedo = !presence[presence.length - 1]
console.log('\n=== analysis ===')
console.log('archived fingerprint:', archivedFp)
console.log('presence transitions:', transitions, '(expected 3: archive/undo/redo)')
console.log('restored after undo:', restoredAfterUndo, '| absent after redo:', absentAfterRedo)
if (transitions > 3) console.log('>>> UNDO/REDO FLASH REPRODUCED (extra toggling)')
else if (restoredAfterUndo && absentAfterRedo) console.log('>>> no flash (undo restored, redo re-removed, clean)')
else console.log('>>> unexpected trajectory (inspect log)')
if (errs.length) console.log('console errors:', JSON.stringify(errs.slice(0, 4)))
await browser.close()
