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
  Recipient,
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
