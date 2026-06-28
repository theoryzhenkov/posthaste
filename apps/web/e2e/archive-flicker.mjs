// Playwright repro for the archive/move "flash" (issue 2): archived messages
// briefly reappearing in the mail list before the view settles.
//
// Drives the REAL dev stack (`just dev web`) over the REAL runtime + Stalwart.
// The bearer token is injected from `<state_root>/daemon.json` (browser dev has
// no embedded token). A requestAnimationFrame loop snapshots the rendered
// message-row fingerprints every frame (painted states) AND a MutationObserver
// records childList mutations on the scroll container (catches sub-frame
// remove+re-add). A flash = a row fingerprint present → absent → present again.
//
// Run (inside the Nix dev shell, with `just dev web` running):
//   node apps/web/e2e/archive-flicker.mjs
//
// Prerequisites:
//   - `just dev web` running (Stalwart + seed + server + Vite on pinned ports).
//   - The dev Stalwart's Archive mailbox must have role "archive" (see
//     tools/dev/stalwart/seed.sh — Stalwart auto-creates Drafts/Sent/Junk/Trash
//     with roles but NOT Archive; set it via JMAP Mailbox/set if missing).
//   - Reset the inbox before runs (move archived messages back to Inbox) so there
//     are enough messages to archive.
import { readFileSync } from 'node:fs'

// Resolve playwright-core from the Nix dev shell (POSTHASTE_PLAYWRIGHT_CLI points
// at playwright-core's cli.js); fall back to a bare 'playwright-core' import.
const PW_CORE =
  process.env.POSTHASTE_PLAYWRIGHT_CLI?.replace(/\/cli\.js$/, '/index.mjs') ??
  'playwright-core'
const { chromium } = await import(PW_CORE)

const URL = process.env.APP_URL || 'http://127.0.0.1:5173'
const STATE_ROOT = process.env.POSTHASTE_STATE_ROOT || 'var/dev/posthaste/state'
const daemon = JSON.parse(readFileSync(`${STATE_ROOT}/daemon.json`, 'utf8'))
const ROWSEL = '.ph-scroll:has([data-message-list-empty]) > div > button'
const SWITCH_TO_INBOX = process.env.ALL_INBOXES !== '1' // default: specific Inbox (All Inboxes has a known staleness gap)

const browser = await chromium.launch({ headless: true })
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } })
const errs = []
page.on(
  'console',
  (m) => m.type() === 'error' && errs.push(m.text().slice(0, 140)),
)

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
if (SWITCH_TO_INBOX) {
  await page.getByText('Inbox', { exact: true }).first().click()
  await page.waitForTimeout(1500)
}

// rAF capture (painted states) + MutationObserver (childList remove+re-add).
await page.evaluate(() => {
  window.__FLASH__ = []
  let last = null
  function snap(tag) {
    const sp = document.querySelector('[data-message-list-empty]')
    const c = sp ? sp.parentElement : null
    const r = c
      ? [...c.querySelectorAll(':scope > div > button')].map((b) =>
          b.textContent.replace(/\s+/g, ' ').trim().slice(0, 20),
        )
      : []
    const key = r.join('|')
    if (tag || key !== last) {
      window.__FLASH__.push({
        t: performance.now().toFixed(0),
        tag: tag || null,
        n: r.length,
        rows: r,
      })
      last = key
    }
  }
  window.__SNAP__ = snap
  window.__MUT__ = []
  const mo = new MutationObserver((muts) => {
    for (const m of muts) {
      if (m.target.closest?.('.ph-scroll')) {
        window.__MUT__.push({
          t: performance.now().toFixed(0),
          added: m.addedNodes.length,
          removed: m.removedNodes.length,
        })
      }
    }
  })
  const c0 = document.querySelector('[data-message-list-empty]')?.parentElement
  if (c0) mo.observe(c0, { childList: true })
  ;(function loop() {
    snap(null)
    requestAnimationFrame(loop)
  })()
})

const N = Number(process.env.ARCHIVES || 3)
for (let i = 1; i <= N; i++) {
  await page.evaluate((n) => window.__SNAP__('archive-' + n), i)
  await page.locator(ROWSEL).first().click()
  await page.waitForSelector('button[aria-label="Archive"]', {
    state: 'visible',
    timeout: 5000,
  })
  await page.locator('button[aria-label="Archive"]').click()
  await page.waitForTimeout(250)
}
await page.waitForTimeout(2000)

const log = await page.evaluate(() => ({
  flash: window.__FLASH__,
  mut: window.__MUT__,
}))
console.log('=== rAF frame log ===')
for (const f of log.flash)
  console.log(
    `  t=${f.t.padStart(8)} ${f.tag ?? ''} | ${f.n}r: ${f.rows.join(' || ')}`,
  )
const removes = log.mut.filter((m) => m.removed > 0).length
const adds = log.mut.filter((m) => m.added > 0).length
console.log('=== MutationObserver ===')
console.log(`  removal events: ${removes} | addition events: ${adds}`)
console.log(
  adds > 0
    ? '>>> FLASH REPRODUCED (row re-added after removal)'
    : '>>> no flash (no row re-added after removal)',
)
if (errs.length)
  console.log('console errors:', JSON.stringify(errs.slice(0, 4)))
await browser.close()
