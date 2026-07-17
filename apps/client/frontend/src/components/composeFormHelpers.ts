/**
 * Pure helpers for the compose overlay: form shape, recipient/sender parsing
 * and formatting, send-input assembly, and from-address option derivation.
 *
 * Extracted from ComposeOverlay so the component holds UI/state wiring while
 * these (React-free, testable) helpers stand on their own.
 *
 * @spec docs/L1-compose#mime-structure
 */
import type { Recipient, ReplyContext, SendMessageInput } from '@/api/types'
import type { MessageDetailResult, SendMessageRequest } from '@/gen'
import { formatRecipient } from '@/composeMessage'

export type { ComposeAttachment, ComposeForm } from '@/composeMessage'
export {
  EMPTY_COMPOSE_FORM as EMPTY_FORM,
  MAX_COMPOSE_ATTACHMENT_BYTES,
  MAX_COMPOSE_ATTACHMENTS,
  MAX_COMPOSE_TOTAL_ATTACHMENT_BYTES,
  buildSendInput,
  composeAttachmentFromFile,
  formatRecipient,
  formatRecipients,
  parseRecipients,
  parseSender,
  readAttachmentForSend,
} from '@/composeMessage'

export interface FromAddressOption {
  sourceId: string
  sourceName: string
  name: string | null
  email: string
  origin: 'configured' | 'identity' | 'cached'
}

/** The slice of an account the compose surfaces need: identity naming plus
 * the sending patterns, merged from the `accounts` row and the account's
 * `accountSettings` answer. */
export interface ComposeAccount {
  id: string
  name: string
  fullName: string | null
  emailPatterns: string[]
}

/** One address-book row for From/recipient derivation (the `senderAddresses`
 * family, re-keyed to the compose vocabulary). */
export interface ComposeSenderAddress {
  sourceId: string
  name: string | null
  email: string
}

/** The wire request for `createDraft`/`updateDraft`/`send`: the assembled
 * compose input pinned to its stable draft identity. Hold options (`sendAt`,
 * `undoWindowSeconds`) are attached by the submission layer. */
export function toSendMessageRequest(
  input: SendMessageInput,
  draftId: string,
): SendMessageRequest {
  return {
    from: input.from,
    to: input.to,
    cc: input.cc,
    bcc: input.bcc,
    subject: input.subject,
    body: input.body,
    inReplyTo: input.inReplyTo,
    references: input.references,
    attachments: input.attachments,
    draftId,
  }
}

export function isConcreteEmailPattern(pattern: string): boolean {
  const trimmed = pattern.trim()
  return (
    trimmed.length > 0 &&
    !trimmed.includes('*') &&
    /^[^@\s]+@[^@\s]+$/.test(trimmed)
  )
}

export function wildcardMatchesEmail(pattern: string, email: string): boolean {
  const trimmed = pattern.trim().toLowerCase()
  const normalizedEmail = email.trim().toLowerCase()
  return trimmed.startsWith('*@') && normalizedEmail.endsWith(trimmed.slice(1))
}

export function optionLabel(option: FromAddressOption): string {
  return option.name ? `${option.name} <${option.email}>` : option.email
}

/** Append a signature to a compose body using the conventional `-- ` separator
 * (RFC 3676 §4.3). The signature lands at the bottom of the body — visible and
 * editable — so the user can adjust or remove it per message. Seeded into fresh
 * compositions only (not resumed drafts), so it is never double-inserted.
 *
 * @spec docs/L1-compose#sender-selection */
export function appendSignature(body: string, signature: string): string {
  return `${body}\n\n-- \n${signature}`
}

/**
 * Insert a signature (with the conventional `-- ` RFC 3676 §4.3 delimiter)
 * ABOVE the quoted block of a reply/forward body — standard top-posting — so
 * the message reads: the user's reply, the signature, the attribution line,
 * then the quote.
 *
 * `quoteBlock` is the exact block the compose seeded (attribution + quote, or
 * the forwarded-message block). When it is absent from the body (not seeded
 * yet, or edited away) the signature is appended at the end instead — a
 * later-seeded quote then lands below it, producing the same final order.
 */
export function insertSignatureAboveQuote(
  body: string,
  signature: string,
  quoteBlock: string | null,
): string {
  const index = quoteBlock ? body.indexOf(quoteBlock) : -1
  if (index < 0) {
    return appendSignature(body, signature)
  }
  const before = body.slice(0, index)
  const separator = before.endsWith('\n\n')
    ? ''
    : before.endsWith('\n')
      ? '\n'
      : '\n\n'
  return `${before}${separator}-- \n${signature}\n\n${body.slice(index)}`
}

/**
 * Format the reply attribution line inserted directly above the `>`-quoted
 * body: `On Mon, Jul 6, 2026, 10:34 AM Theo Ryzhenkov <theor@theor.net> wrote:`.
 *
 * The RFC 3339 `date` from the reply-context is localized with `Intl` (the
 * user's locale/timezone by default; pin both in tests for determinism). The
 * name falls back to the bare email when the sender has no display name; a
 * missing or unparseable date degrades to `<sender> wrote:`; no sender at all
 * yields no attribution line (null).
 */
export function formatReplyAttribution(
  from: Recipient | null,
  date: string | null,
  options?: { locale?: string; timeZone?: string },
): string | null {
  if (!from) {
    return null
  }
  const sender = formatRecipient(from)
  const parsed = date ? new Date(date) : null
  if (!parsed || Number.isNaN(parsed.getTime())) {
    return `${sender} wrote:`
  }
  const formatted = new Intl.DateTimeFormat(options?.locale, {
    weekday: 'short',
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
    timeZone: options?.timeZone,
  }).format(parsed)
  return `On ${formatted} ${sender} wrote:`
}

/** `>`-prefix every line of a body for reply quoting. Mirrors the engine's
 *  `reply.rs::quote_body` so a cache-seeded quote matches the served one. */
function quoteBodyLines(body: string): string {
  return body
    .split('\n')
    .map((line) => `> ${line}`)
    .join('\n')
}

/** Prefix a subject unless it already carries the prefix (case-insensitive).
 *  Mirrors the engine's `compose.rs::prefix_subject`. */
function prefixSubject(prefix: string, subject: string): string {
  return subject.toLowerCase().startsWith(prefix.toLowerCase())
    ? subject
    : `${prefix} ${subject}`
}

/** The forwarded-message block a forward compose seeds below the cursor:
 * a conventional header naming the original sender/date/subject/recipients,
 * then the original text body verbatim. */
function forwardedBlock(detail: MessageDetailResult): string | null {
  const bodyText = detail.bodyText
  if (typeof bodyText !== 'string' || bodyText.length === 0) {
    return null
  }
  const summary = detail.summary
  const fromLine = summary.fromName
    ? `${summary.fromName}${summary.fromEmail ? ` <${summary.fromEmail}>` : ''}`
    : (summary.fromEmail ?? 'Unknown sender')
  const toLine = summary.to
    .map((recipient) => formatRecipient(recipient))
    .join(', ')
  const lines = [
    '---------- Forwarded message ----------',
    `From: ${fromLine}`,
    `Date: ${summary.receivedAt}`,
    `Subject: ${summary.subject ?? '(no subject)'}`,
  ]
  if (toLine) {
    lines.push(`To: ${toLine}`)
  }
  return `${lines.join('\n')}\n\n${bodyText}`
}

/**
 * Build the {@link ReplyContext} for a reply/reply-all/forward compose from
 * the anchored message's `messageDetail` answer — threading headers from the
 * summary, the quote/forward block from the inline text body.
 *
 * Derivation limits of the detail projection: the original `Cc` list is not
 * served (reply-all covers From + To only), and the original `References`
 * chain is approximated from the parent pointer (`inReplyTo`) plus the
 * message's own `Message-ID` — enough for conventional threaders.
 */
export function replyContextFromDetail(
  detail: MessageDetailResult,
): ReplyContext {
  const summary = detail.summary
  const bodyText =
    typeof detail.bodyText === 'string' && detail.bodyText.length > 0
      ? detail.bodyText
      : null
  const subject = summary.subject ?? '(no subject)'
  const originalFrom: Recipient[] = summary.fromEmail
    ? [{ name: summary.fromName ?? null, email: summary.fromEmail }]
    : []
  const references =
    [summary.inReplyTo, summary.rfcMessageId]
      .filter((id): id is string => Boolean(id))
      .join(' ') || null
  return {
    // A reply addresses the original sender.
    to: originalFrom,
    // The detail projection carries no Cc list; reply-all spans From + To.
    cc: [],
    originalTo: summary.to,
    replySubject: prefixSubject('Re:', subject),
    forwardSubject: prefixSubject('Fwd:', subject),
    quotedBody: bodyText ? quoteBodyLines(bodyText) : null,
    forwardedBody: forwardedBlock(detail),
    inReplyTo: summary.rfcMessageId ?? null,
    references,
    originalFrom,
    // The summary carries the received date; the attribution line uses it.
    originalDate: summary.receivedAt,
  }
}

export function accountFromOptions(
  accounts: ComposeAccount[],
  identity: Recipient | null,
  identitySourceId: string,
  cachedSenders: ComposeSenderAddress[],
): FromAddressOption[] {
  const byAccount = new Map(accounts.map((account) => [account.id, account]))
  const options: FromAddressOption[] = []

  for (const account of accounts) {
    for (const email of account.emailPatterns.filter(isConcreteEmailPattern)) {
      options.push({
        sourceId: account.id,
        sourceName: account.name,
        name: account.fullName,
        email,
        origin: 'configured',
      })
    }
  }

  if (identity) {
    options.unshift({
      sourceId: identitySourceId,
      sourceName: byAccount.get(identitySourceId)?.name ?? identitySourceId,
      name: identity.name,
      email: identity.email,
      origin: 'identity',
    })
  }

  for (const cached of cachedSenders) {
    const account = byAccount.get(cached.sourceId)
    if (!account) {
      continue
    }
    // The `senderAddresses` view is now the full address book (every
    // correspondent, senders AND recipients), not just addresses this user has
    // sent *from*. The From selector must only offer the user's own sending
    // identities, so keep a cached address only when it falls inside one of the
    // account's own email patterns (a concrete identity or a `*@domain`
    // catch-all) — external correspondents are excluded.
    const isOwnIdentity = account.emailPatterns.some((pattern) =>
      isConcreteEmailPattern(pattern)
        ? pattern.trim().toLowerCase() === cached.email.trim().toLowerCase()
        : wildcardMatchesEmail(pattern, cached.email),
    )
    if (!isOwnIdentity) {
      continue
    }
    options.push({
      sourceId: cached.sourceId,
      sourceName: account.name,
      name: cached.name,
      email: cached.email,
      origin: 'cached',
    })
  }

  const seen = new Set<string>()
  return options.filter((option) => {
    const key = `${option.sourceId}:${option.email.toLowerCase()}`
    if (seen.has(key)) {
      return false
    }
    seen.add(key)
    return true
  })
}
