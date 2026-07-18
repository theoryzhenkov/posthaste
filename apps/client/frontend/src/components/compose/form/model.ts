/**
 * Pure helpers for the compose overlay: form shape, recipient/sender parsing
 * and formatting, send-input assembly, and from-address option derivation.
 *
 * Extracted from ComposeOverlay so the component holds UI/state wiring while
 * these (React-free, testable) helpers stand on their own.
 *
 */
import type { Recipient, ReplyContext, SendMessageInput } from '@/data/transport/api'
import type { MessageDetailResult, SendMessageRequest } from '@/gen'
import {
  patternEmailAddress,
  patternMatchesEmail,
  type EmailPattern,
} from '@/domain/address'
import type { ComposeIntent, MailtoSeed } from '@/domain/composeIntent'
import {
  EMPTY_COMPOSE_FORM,
  formatRecipient,
  type ComposeForm,
} from '@/components/compose/form/composeMessage'

export type { ComposeAttachment, ComposeForm } from '@/components/compose/form/composeMessage'
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
} from '@/components/compose/form/composeMessage'

export interface FromAddressOption {
  sourceId: string
  sourceName: string
  name: string | null
  email: string
  origin: 'configured' | 'identity' | 'cached'
}

/** The slice of an account the compose surfaces need: identity naming plus
 * the sending patterns (parsed ONCE at the query boundary), merged from the
 * `accounts` row and the account's `accountSettings` answer. */
export interface ComposeAccount {
  id: string
  name: string
  fullName: string | null
  emailPatterns: readonly EmailPattern[]
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

export function optionLabel(option: FromAddressOption): string {
  return option.name ? `${option.name} <${option.email}>` : option.email
}

/** Append a signature to a compose body using the conventional `-- ` separator
 * (RFC 3676 §4.3). The signature lands at the bottom of the body — visible and
 * editable — so the user can adjust or remove it per message. Seeded into fresh
 * compositions only (not resumed drafts), so it is never double-inserted. */
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
function formatReplyAttribution(
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

/** The compose form seeded synchronously when the composer opens: a resumed
 * draft's loaded content, a mailto's parsed fields, or (for every other kind)
 * the empty form — a reply/forward starts empty and streams its seed in later
 * (see {@link deriveReplySeed}). */
export function initialComposeForm({
  draftSeed,
  intentKind,
  mailtoSeed,
}: {
  draftSeed: DraftSeed | undefined
  intentKind: ComposeIntent['kind']
  mailtoSeed: MailtoSeed | undefined
}): ComposeForm {
  if (intentKind === 'draft') {
    return draftSeed ? { ...draftSeed, attachments: [] } : EMPTY_COMPOSE_FORM
  }
  // A mailto compose is a `new` message whose fields are already known —
  // seed them synchronously (nothing streams in later).
  if (intentKind === 'mailto' && mailtoSeed) {
    return {
      ...EMPTY_COMPOSE_FORM,
      to: mailtoSeed.to,
      subject: mailtoSeed.subject,
      body: mailtoSeed.body,
    }
  }
  return EMPTY_COMPOSE_FORM
}

/** A resumed draft's loaded field values (no attachments — those seed
 * separately through the forward-attachment path). */
export interface DraftSeed {
  from: string
  to: string
  cc: string
  bcc: string
  subject: string
  body: string
}

/**
 * Derive the reply-all recipient set: original From + To (minus self) go to
 * `to`, original Cc (minus self) goes to `cc`. Recipients are de-duplicated by
 * email (case-insensitive). Only the primary identity address is excluded;
 * alias exclusion is a follow-up.
 */
export function replyAllRecipients(
  replyTo: Recipient[],
  originalTo: Recipient[],
  cc: Recipient[],
  selfEmail: string | undefined,
): { to: Recipient[]; cc: Recipient[] } {
  const self = selfEmail?.toLowerCase()
  const dedupedExcludingSelf = (recipients: Recipient[]): Recipient[] => {
    const seen = new Set<string>()
    const out: Recipient[] = []
    for (const r of recipients) {
      const key = r.email.toLowerCase()
      if (seen.has(key) || (self && key === self)) continue
      seen.add(key)
      out.push(r)
    }
    return out
  }
  return {
    to: dedupedExcludingSelf([...replyTo, ...originalTo]),
    cc: dedupedExcludingSelf(cc),
  }
}

/** What a reply/reply-all/forward streams into the (already interactive)
 * form once its anchored message's context arrives. */
export interface ReplySeed {
  to: Recipient[]
  cc: Recipient[]
  subject: string
  /** The exact quote block appended below any early-typed text: attribution +
   * `>`-quote for a reply, the forwarded-message block for a forward. */
  quoteBlock: string | null
}

/**
 * Build the {@link ReplySeed} for a message-anchored compose. A reply's quote
 * is headed by the localized attribution line ("On <date> <sender> wrote:");
 * a forward's block carries its own header. Reply-all derives the full
 * recipient set (original From + To, plus the original Cc) with the user's
 * own address excluded; a plain reply uses the original From only; forward
 * starts unaddressed.
 */
export function deriveReplySeed(
  intentKind: 'reply' | 'replyAll' | 'forward',
  replyContext: ReplyContext,
  selfEmail: string | undefined,
): ReplySeed {
  const attribution =
    intentKind === 'forward'
      ? null
      : formatReplyAttribution(
          replyContext.originalFrom[0] ?? replyContext.to[0] ?? null,
          replyContext.originalDate,
        )
  const quotedWithAttribution = replyContext.quotedBody
    ? attribution
      ? `${attribution}\n${replyContext.quotedBody}`
      : replyContext.quotedBody
    : null
  const { to, cc } =
    intentKind === 'forward'
      ? { to: [], cc: [] }
      : intentKind === 'replyAll'
        ? replyAllRecipients(
            replyContext.to,
            replyContext.originalTo,
            replyContext.cc,
            selfEmail,
          )
        : { to: replyContext.to, cc: [] }
  return {
    to,
    cc,
    subject:
      intentKind === 'forward'
        ? replyContext.forwardSubject
        : replyContext.replySubject,
    quoteBlock:
      intentKind === 'forward'
        ? replyContext.forwardedBody
        : quotedWithAttribution,
  }
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
    for (const pattern of account.emailPatterns) {
      const email = patternEmailAddress(pattern)
      if (!email) {
        continue
      }
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
      patternMatchesEmail(pattern, cached.email),
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
