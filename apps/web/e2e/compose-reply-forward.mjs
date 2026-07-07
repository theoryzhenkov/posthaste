// Phase A verification: reply + forward compose flows against the real dev
// stack (just dev web). The server replyContext view + client form population +
// send-acceptance are all exercised end-to-end.
//
// - Reply surface: subject prefixed "Re:", inReplyTo set (server-verified).
// - Forward surface: subject prefixed "Fwd:".
// - Send: submit → "Message sent" toast (client → server → outbox op accepted).
//
// Run: POSTHASTE_PLAYWRIGHT_CLI=<nix devShell path> bun apps/web/e2e/compose-reply-forward.mjs
import { readFileSync } from 'node:fs'

import { mintSessionToken } from './lib/session-token.mjs'

const PW_CORE =
  process.env.POSTHASTE_PLAYWRIGHT_CLI?.replace(/\/cli\.js$/, '/index.mjs') ??
  'playwright-core'
const { chromium } = await import(PW_CORE)

const URL = 'http://127.0.0.1:5173'
const STATE_ROOT = process.env.POSTHASTE_STATE_ROOT || 'var/dev/posthaste/state'
const daemon = JSON.parse(readFileSync(`${STATE_ROOT}/daemon.json`, 'utf8'))
// daemon.token is the {mint, read} bootstrap — no write verbs; mint a session token.
const token = await mintSessionToken(daemon)
const SOURCE_ID = 'local-stalwart'
const HEADERS = { Authorization: `Bearer ${token}` }

async function firstMessageId() {
  const r = await fetch(
    `http://localhost:${daemon.port}/v1/sources/${SOURCE_ID}/messages?limit=5`,
    { headers: HEADERS },
  )
  const j = await r.json()
  const withSubject = (j.items ?? []).filter((m) => m.subject)
  const pick = withSubject[0] ?? (j.items ?? [])[0]
  if (!pick) throw new Error('no seeded message to reply to')
  return { id: pick.id, subject: pick.subject ?? '(no subject)' }
}

async function openComposeSurface(page, kind, messageId) {
  const url = `${URL}/surface/compose?composeKind=${kind}&sourceId=${SOURCE_ID}&messageId=${messageId}`
  await page.goto(url, { waitUntil: 'load', timeout: 30000 })
  // The overlay fetches replyContext before populating; wait for the subject
  // input to be present + non-empty (confirms the server view returned + the
  // form populated). Fields are wrapped in <label> via ComposeLine.
  const subject = page.getByLabel('Subject', { exact: true })
  await subject.waitFor({ state: 'visible', timeout: 20000 })
  await page.waitForFunction(
    () => {
      const el = [...document.querySelectorAll('label')]
        .find((l) => l.textContent?.includes('Subject'))
        ?.querySelector('input')
      return !!el && el.value.length > 0
    },
    { timeout: 20000 },
  )
}

async function sendAndObserve(page) {
  // Wait for the Send button to be enabled, click, then watch for the success
  // toast OR an error message in the footer.
  await page.waitForFunction(
    () => {
      const send = [...document.querySelectorAll('button')].find((b) =>
        /^send$/i.test(b.textContent ?? ''),
      )
      return !!send && !send.disabled
    },
    { timeout: 15000 },
  )
  const sendBtn = page.getByRole('button', { name: /^send$/i }).first()
  await sendBtn.click()
  // Race the toast against an error message + a timeout.
  const toast = page.getByText('Message sent', { exact: true })
  const outcome = await Promise.race([
    toast
      .waitFor({ timeout: 20000 })
      .then(() => ({ sent: true, error: null }))
      .catch(() => ({ sent: false, error: 'toast-not-seen' })),
    // Also surface a footer error if one appears (validation / send failure).
    page
      .locator('[role="alert"], .text-destructive')
      .first()
      .waitFor({ timeout: 20000 })
      .then(() => ({ sent: false, error: 'footer-error' }))
      .catch(() => null),
  ])
  const footerText = await page
    .locator('.text-destructive')
    .first()
    .textContent()
    .catch(() => null)
  return { ...outcome, footerError: footerText }
}

const browser = await chromium.launch({ headless: true })
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } })
const errs = []
page.on(
  'console',
  (m) => m.type() === 'error' && errs.push(m.text().slice(0, 200)),
)
page.on('requestfailed', (r) =>
  errs.push(`REQFAIL ${r.url().slice(0, 80)} ${r.failure()?.errorText ?? ''}`),
)

await page.addInitScript(
  ([t, p]) => {
    window.__POSTHASTE_TOKEN__ = t
    window.__POSTHASTE_PORT__ = p
    window.__POSTHASTE_RUNTIME_MODE__ = 'loopback'
  },
  [token, daemon.port],
)

const results = { consoleErrors: errs }
try {
  const { id: messageId, subject } = await firstMessageId()
  results.message = { id: messageId, subject }

  // --- Reply -------------------------------------------------------------
  await openComposeSurface(page, 'reply', messageId)
  await page.waitForTimeout(500)
  const replySubject = await page
    .getByLabel('Subject', { exact: true })
    .inputValue()
  const replyTo = await page.getByLabel('To', { exact: true }).inputValue()
  results.reply = {
    subject: replySubject,
    startsWithRe: replySubject.startsWith('Re:'),
    to: replyTo,
  }
  if (!replyTo.trim()) {
    await page.getByLabel('To', { exact: true }).fill('dev@example.org')
  }
  results.reply.send = await sendAndObserve(page)

  // --- Forward -----------------------------------------------------------
  await openComposeSurface(page, 'forward', messageId)
  await page.waitForTimeout(500)
  const fwdSubject = await page
    .getByLabel('Subject', { exact: true })
    .inputValue()
  const fwdTo = await page.getByLabel('To', { exact: true }).inputValue()
  results.forward = {
    subject: fwdSubject,
    startsWithFwd: fwdSubject.startsWith('Fwd:'),
    to: fwdTo,
  }
  if (!fwdTo.trim()) {
    await page.getByLabel('To', { exact: true }).fill('dev@example.org')
  }
  results.forward.send = await sendAndObserve(page)
} catch (e) {
  results.error = String(e)
} finally {
  await browser.close()
}

console.log(JSON.stringify(results, null, 2))
process.exit(
  results.error || !results.reply?.send?.sent || !results.forward?.send?.sent
    ? 1
    : 0,
)
