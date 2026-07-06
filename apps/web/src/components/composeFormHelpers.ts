/**
 * Pure helpers for the compose overlay: form shape, recipient/sender parsing
 * and formatting, send-input assembly, and from-address option derivation.
 *
 * Extracted from ComposeOverlay so the component holds UI/state wiring while
 * these (React-free, testable) helpers stand on their own.
 *
 * @spec docs/L1-compose#mime-structure
 */
import type {
  AccountOverview,
  CachedSenderAddress,
  MessageDetail,
  Recipient,
  ReplyContext,
} from '@/api/types'

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

/**
 * FIX2 — build a PLAIN-reply {@link ReplyContext} from a message already in the
 * detail-pane cache ({@link MessageDetail}), so the reply composer can show the
 * quoted body + reply recipient + subject INSTANTLY without waiting on the fresh
 * `replyContext` Email/get round-trip.
 *
 * Only safe for a plain reply: the cached detail carries the text body, the
 * original From (→ the reply recipient) + To, and the source `Message-ID`
 * (→ `In-Reply-To`), but NOT the `References` header or the `Cc` list. So this
 * seeds a provisional context for DISPLAY only; the authoritative fetch still
 * runs and supplies `references` (+ `cc`) for the actual send/save — which the
 * composer gates on (`isPlaceholderData`). Returns undefined when the body is
 * not cached (the composer then streams the served quote in instead).
 */
export function replyContextFromCachedMessage(
  detail: MessageDetail,
): ReplyContext | undefined {
  const bodyText = detail.bodyText
  if (typeof bodyText !== 'string' || bodyText.length === 0) {
    return undefined
  }
  const subject = detail.subject ?? '(no subject)'
  const originalFrom: Recipient[] = detail.fromEmail
    ? [{ name: detail.fromName ?? null, email: detail.fromEmail }]
    : []
  return {
    // A plain reply addresses the original sender.
    to: originalFrom,
    // Not in the cache; the authoritative fetch fills it (and a plain reply
    // does not use Cc anyway).
    cc: [],
    originalTo: detail.to ?? [],
    replySubject: prefixSubject('Re:', subject),
    forwardSubject: prefixSubject('Fwd:', subject),
    quotedBody: quoteBodyLines(bodyText),
    // Forward isn't cache-seeded (it needs the served forwarded-body block).
    forwardedBody: null,
    inReplyTo: detail.rfcMessageId ?? null,
    // Not in the cache; the served context supplies real threading before send.
    references: null,
  }
}

export function accountFromOptions(
  accounts: AccountOverview[],
  identity: Recipient | null,
  identitySourceId: string,
  cachedSenders: CachedSenderAddress[],
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
