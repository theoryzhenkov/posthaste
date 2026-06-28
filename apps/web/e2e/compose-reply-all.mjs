// Phase B verification: reply-all compose flow against the real dev stack.
// Sends a message with multiple recipients (From=dev, To=alice+bob, Cc=carol),
// then opens the replyAll surface + verifies the client-derived recipient set:
//   to = original From + To, minus self (dev)  → alice, bob
//   cc = original Cc, minus self               → carol
// Then sends + verifies the toast.
//
// Run: POSTHASTE_PLAYWRIGHT_CLI=<nix devShell path> bun apps/web/e2e/compose-reply-all.mjs
import { readFileSync } from 'node:fs'

const PW_CORE =
  process.env.POSTHASTE_PLAYWRIGHT_CLI?.replace(/\/cli\.js$/, '/index.mjs') ??
  'playwright-core'
const { chromium } = await import(PW_CORE)

const URL = 'http://127.0.0.1:5173'
const STATE_ROOT = process.env.POSTHASTE_STATE_ROOT || 'var/dev/posthaste/state'
const daemon = JSON.parse(readFileSync(`${STATE_ROOT}/daemon.json`, 'utf8'))
const SOURCE_ID = 'local-stalwart'
const HEADERS = {
  Authorization: `Bearer ${daemon.token}`,
  'Content-Type': 'application/json',
}
const SELF_EMAIL = 'dev@example.org'

const SUBJECT = `reply-all probe ${Date.now()}`
const RECIPIENTS = {
  from: { name: 'Posthaste dev mailbox', email: SELF_EMAIL },
  to: [
    { name: null, email: 'alice@example.com' },
    { name: null, email: 'bob@example.com' },
  ],
  cc: [{ name: null, email: 'carol@example.com' }],
  bcc: [],
  subject: SUBJECT,
  body: 'testing reply-all recipient derivation',
  inReplyTo: null,
  references: null,
  attachments: [],
}

async function sendProbeMessage() {
  const r = await fetch(
    `http://localhost:${daemon.port}/v1/sources/${SOURCE_ID}/commands/send`,
    { method: 'POST', headers: HEADERS, body: JSON.stringify(RECIPIENTS) },
  )
  if (!r.ok) throw new Error(`send failed: ${r.status} ${await r.text()}`)
  // Poll for the sent message to appear (the outbox processes asynchronously).
  for (let i = 0; i < 30; i++) {
    await new Promise((res) => setTimeout(res, 500))
    const m = await fetch(
      `http://localhost:${daemon.port}/v1/sources/${SOURCE_ID}/messages?limit=20`,
      { headers: HEADERS },
    ).then((r) => r.json())
    const sent = (m.items ?? []).find((x) => x.subject === SUBJECT)
    if (sent) return sent.id
  }
  throw new Error('probe message did not appear in the list')
}

const browser = await chromium.launch({ headless: true })
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } })
const errs = []
page.on(
  'console',
  (m) => m.type() === 'error' && errs.push(m.text().slice(0, 160)),
)

await page.addInitScript(
  ([t, p]) => {
    window.__POSTHASTE_TOKEN__ = t
    window.__POSTHASTE_PORT__ = p
    window.__POSTHASTE_RUNTIME_MODE__ = 'loopback'
  },
  [daemon.token, daemon.port],
)

const results = { consoleErrors: errs }
try {
  const messageId = await sendProbeMessage()
  results.messageId = messageId

  // Open the replyAll surface.
  await page.goto(
    `${URL}/surface/compose?composeKind=replyAll&sourceId=${SOURCE_ID}&messageId=${messageId}`,
    { waitUntil: 'load', timeout: 30000 },
  )
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
  await page.waitForTimeout(500)

  const subjectVal = await subject.inputValue()
  const toVal = await page.getByLabel('To', { exact: true }).inputValue()
  const ccVal = await page.getByLabel('Cc', { exact: true }).inputValue()
  const toEmails = toVal
    .split(/[;,]/)
    .map((s) => s.trim())
    .filter(Boolean)
  const ccEmails = ccVal
    .split(/[;,]/)
    .map((s) => s.trim())
    .filter(Boolean)

  results.replyAll = {
    subject: subjectVal,
    startsWithRe: subjectVal.startsWith('Re:'),
    to: toVal,
    toEmails,
    // Expected: alice + bob (From=dev is self, excluded; original To = alice+bob).
    toHasAlice: toEmails.some((e) => e.endsWith('alice@example.com')),
    toHasBob: toEmails.some((e) => e.endsWith('bob@example.com')),
    toExcludesSelf: !toEmails.some((e) => e.toLowerCase() === SELF_EMAIL),
    cc: ccVal,
    ccEmails,
    ccHasCarol: ccEmails.some((e) => e.endsWith('carol@example.com')),
    ccExcludesSelf: !ccEmails.some((e) => e.toLowerCase() === SELF_EMAIL),
  }

  // Send (recipients are already populated by the reply-all derivation).
  await page.waitForFunction(
    () => {
      const send = [...document.querySelectorAll('button')].find((b) =>
        /^send$/i.test(b.textContent ?? ''),
      )
      return !!send && !send.disabled
    },
    { timeout: 15000 },
  )
  await page
    .getByRole('button', { name: /^send$/i })
    .first()
    .click()
  results.replyAll.sentToast = await page
    .getByText('Message sent', { exact: true })
    .waitFor({ timeout: 20000 })
    .then(() => true)
    .catch(() => false)
} catch (e) {
  results.error = String(e)
} finally {
  await browser.close()
}

console.log(JSON.stringify(results, null, 2))
process.exit(
  results.error ||
    !results.replyAll?.startsWithRe ||
    !results.replyAll?.toHasAlice ||
    !results.replyAll?.toHasBob ||
    !results.replyAll?.toExcludesSelf ||
    !results.replyAll?.ccHasCarol ||
    !results.replyAll?.sentToast
    ? 1
    : 0,
)
