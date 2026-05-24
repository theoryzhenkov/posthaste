import type {
  AccountOverview,
  ConversationPage,
  ConversationSummary,
} from './api/types'

export interface AddressSuggestionOption {
  name: string | null
  email: string
  sourceLabel: string
  origin: 'account' | 'correspondent'
}

export function isConcreteEmailAddress(pattern: string): boolean {
  const trimmed = pattern.trim()
  return (
    trimmed.length > 0 &&
    !trimmed.includes('*') &&
    /^[^@\s]+@[^@\s]+$/.test(trimmed)
  )
}

export function formatAddressSuggestion(
  option: AddressSuggestionOption,
): string {
  return option.name ? `${option.name} <${option.email}>` : option.email
}

export function buildRecipientSuggestionOptions(
  accounts: AccountOverview[],
  conversationPages: Array<ConversationPage | undefined>,
): AddressSuggestionOption[] {
  const suggestions: AddressSuggestionOption[] = []

  for (const account of accounts) {
    for (const email of account.emailPatterns.filter(isConcreteEmailAddress)) {
      suggestions.push({
        name: account.fullName,
        email,
        sourceLabel: account.name,
        origin: 'account',
      })
    }
  }

  for (const page of conversationPages) {
    for (const conversation of page?.items ?? []) {
      const correspondent = conversationToSuggestion(conversation)
      if (correspondent) {
        suggestions.push(correspondent)
      }
    }
  }

  const seen = new Set<string>()
  return suggestions.filter((suggestion) => {
    const key = suggestion.email.trim().toLowerCase()
    if (!key || seen.has(key)) {
      return false
    }
    seen.add(key)
    return true
  })
}

export function filterAddressSuggestions(
  options: AddressSuggestionOption[],
  value: string,
  limit = 8,
): AddressSuggestionOption[] {
  const needle = currentAddressToken(value).toLowerCase()
  const filtered = needle
    ? options.filter((option) => {
        const label = formatAddressSuggestion(option).toLowerCase()
        return (
          option.email.toLowerCase().includes(needle) ||
          option.name?.toLowerCase().includes(needle) ||
          option.sourceLabel.toLowerCase().includes(needle) ||
          label.includes(needle)
        )
      })
    : options

  return filtered.slice(0, limit)
}

export function insertAddressSuggestion(
  value: string,
  suggestion: AddressSuggestionOption,
): string {
  const { start, end } = currentAddressTokenBounds(value)
  const rawPrefix = value.slice(0, start)
  const prefix =
    rawPrefix.length > 0 && !/\s$/.test(rawPrefix) ? `${rawPrefix} ` : rawPrefix
  const suffix = value.slice(end).replace(/^\s+/, '')
  const separator = suffix.length > 0 ? ' ' : ', '
  return `${prefix}${formatAddressSuggestion(suggestion)}${separator}${suffix}`
}

function conversationToSuggestion(
  conversation: ConversationSummary,
): AddressSuggestionOption | null {
  const email = conversation.fromEmail?.trim()
  if (!email) {
    return null
  }
  return {
    name: conversation.fromName,
    email,
    sourceLabel:
      conversation.latestSourceName || conversation.sourceNames[0] || 'Recent',
    origin: 'correspondent',
  }
}

function currentAddressToken(value: string): string {
  const { start, end } = currentAddressTokenBounds(value)
  return value.slice(start, end).trim().replace(/^"|"$/g, '')
}

function currentAddressTokenBounds(value: string): {
  start: number
  end: number
} {
  const lastComma = value.lastIndexOf(',')
  const lastSemicolon = value.lastIndexOf(';')
  const start = Math.max(lastComma, lastSemicolon) + 1
  return { start, end: value.length }
}
