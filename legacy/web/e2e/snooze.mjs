// Playwright e2e for snooze (Slice 5): snoozing a message moves it out of the
// Inbox, + undoing restores it. Drives the REAL dev stack (`just dev web`).
//
// Setup designates a Snoozed mailbox via the API (PATCH an existing mailbox's
// role to "snooze" — no gateway create_mailbox exists), then exercises the UI:
// select an Inbox message → Snooze button → "Tomorrow" preset → assert it left
// the Inbox → toast Undo → assert it returns. Teardown restores the role.
import { readFileSync } from 'node:fs'

import { mintSessionToken } from './lib/session-token.mjs'

const PW_CORE =
  process.env.POSTHASTE_PLAYWRIGHT_CLI?.replace(/\/cli\.js$/, '/index.mjs') ??
  'playwright-core'
const { chromium } = await import(PW_CORE)

const URL = 'http://127.0.0.1:5173'
const STATE_ROOT = process.env.POSTHASTE_STATE_ROOT || 'var/dev/posthaste/state'
const ACCOUNT = 'local-stalwart'
// "Junk Mail" — repurposed as the Snoozed mailbox for this run, restored after.
const SNOOZE_MAILBOX_ID = 'c'
const SNOOZE_MAILBOX_RESTORE_ROLE = 'junk'
const daemon = JSON.parse(readFileSync(`${STATE_ROOT}/daemon.json`, 'utf8'))
// daemon.token is the {mint, read} bootstrap — no write verbs; mint a session token.
const token = await mintSessionToken(daemon)
const API = `http://127.0.0.1:${daemon.port}/v1`
const auth = { Authorization: `Bearer ${token}` }
const ROWSEL = '.ph-scroll:has([data-message-list-empty]) > div > button'

async function setMailboxRole(role) {
  const res = await fetch(
    `${API}/sources/${ACCOUNT}/mailboxes/${SNOOZE_MAILBOX_ID}`,
    {
      method: 'PATCH',
      headers: { ...auth, 'Content-Type': 'application/json' },
      body: JSON.stringify({ role }),
    },
  )
  if (!res.ok) throw new Error(`PATCH mailbox role ${role}: ${res.status}`)
  await res.json()
}

async function inboxRows(page) {
  return page.evaluate((sel) => {
    const c = document.querySelector('[data-message-list-empty]')?.parentElement
    return c
      ? [...c.querySelectorAll(`:scope > div > button`)].map((b) =>
          b.textContent.replace(/\s+/g, ' ').trim().slice(0, 24),
        )
      : []
  }, ROWSEL)
}

const browser = await chromium.launch({ headless: true })
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } })
const errs = []
page.on('console', (m) => {
  const t = m.text()
  if (m.type() === 'error') errs.push(t.slice(0, 200))
  if (/snooze|mutation|applyDiff|captureMutation|userInitiated/i.test(t))
    console.log('  [console]', m.type(), t.slice(0, 160))
})
page.on('pageerror', (e) =>
  console.log('  [pageerror]', e.message.slice(0, 200)),
)

try {
  // Setup: designate the Snoozed mailbox.
  await setMailboxRole('snooze')

  await page.addInitScript(
    ([t, p]) => {
      window.__POSTHASTE_TOKEN__ = t
      window.__POSTHASTE_PORT__ = p
      window.__POSTHASTE_RUNTIME_MODE__ = 'loopback'
    },
    [token, daemon.port],
  )
  await page.goto(URL, { waitUntil: 'load', timeout: 30000 })
  await page.waitForSelector(ROWSEL, { timeout: 20000 })
  await page.waitForTimeout(1500)
  await page.getByText('Inbox', { exact: true }).first().click()
  await page.waitForTimeout(1500)

  const before = await inboxRows(page)
  if (before.length === 0) throw new Error('Inbox has no messages to snooze')
  const snoozedSubject = before[0]
  console.log('snooze target (first Inbox row):', snoozedSubject)

  // Select the first message → the detail header shows the Snooze button.
  // (There are two "Snooze" buttons — the message-list quick action + the
  // detail-header popover trigger; target the latter via its popover role.)
  await page.locator(ROWSEL).first().click()
  await page.waitForTimeout(800)
  await page
    .locator('[data-slot="popover-trigger"][aria-label="Snooze"]')
    .click()
  await page.waitForTimeout(500)
  await page.screenshot({ path: '/tmp/snooze-popover.png' })
  const tomorrow = page.getByRole('button', { name: 'Tomorrow' })
  console.log(
    'Tomorrow button visible:',
    await tomorrow.isVisible().catch(() => false),
  )
  await tomorrow.click()
  // Let the optimistic move + settlement settle.
  await page.waitForTimeout(2500)

  const afterSnooze = await inboxRows(page)
  const leftInbox = !afterSnooze.some((r) => r === snoozedSubject)
  console.log(
    'after snooze — target left Inbox:',
    leftInbox,
    '(rows:',
    afterSnooze.length,
    'was',
    before.length,
    ')',
  )

  // Undo via the toast.
  await page.getByRole('button', { name: 'Undo' }).click()
  await page.waitForTimeout(2500)

  const afterUndo = await inboxRows(page)
  const backInInbox = afterUndo.some((r) => r === snoozedSubject)
  console.log(
    'after undo — target back in Inbox:',
    backInInbox,
    '(rows:',
    afterUndo.length,
    ')',
  )

  console.log('\n=== SNOOZE E2E ===')
  console.log('snooze left the Inbox:', leftInbox)
  console.log('undo restored it:', backInInbox)
  console.log('console errors:', errs.length)
  if (errs.length) console.log(errs.slice(0, 5).join('\n'))
  console.log('result:', leftInbox && backInInbox ? 'PASS' : 'FAIL')
} finally {
  // Teardown: restore the mailbox's original role.
  try {
    await setMailboxRole(SNOOZE_MAILBOX_RESTORE_ROLE)
  } catch (e) {
    console.error('teardown role restore failed:', e.message)
  }
  await browser.close()
}

if (errs.length) process.exitCode = 1
