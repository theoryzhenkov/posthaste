/**
 * Email address/pattern parsing (the R3 boundary) plus the shared
 * address-autocomplete engine built on it.
 */

/** A concrete, deliverable address (`local@domain`, trimmed, no wildcard). */
export type EmailAddress = string & { readonly __brand: 'EmailAddress' }

/** A sending-identity pattern from account settings: a concrete
 *  {@link EmailAddress} or a `*@domain` catch-all (trimmed). */
export type EmailPattern = string & { readonly __brand: 'EmailPattern' }

const CONCRETE_ADDRESS = /^[^@\s]+@[^@\s]+$/
const WILDCARD_PATTERN = /^\*@[^@\s*]+$/

/** Parse raw text into a concrete address, or `null` (empty, wildcard, or
 *  malformed input). The ONE home of the concrete-address rule. */
export function parseEmailAddress(raw: string): EmailAddress | null {
  const trimmed = raw.trim()
  return trimmed.length > 0 &&
    !trimmed.includes('*') &&
    CONCRETE_ADDRESS.test(trimmed)
    ? (trimmed as EmailAddress)
    : null
}

/** Parse raw text into a sending pattern: a concrete address or a `*@domain`
 *  catch-all. Anything else (empty, other wildcard shapes) is `null`. */
export function parseEmailPattern(raw: string): EmailPattern | null {
  const trimmed = raw.trim()
  if (parseEmailAddress(trimmed) || WILDCARD_PATTERN.test(trimmed)) {
    return trimmed as EmailPattern
  }
  return null
}

/** The concrete address a pattern names, or `null` for a catch-all. */
export function patternEmailAddress(
  pattern: EmailPattern,
): EmailAddress | null {
  return parseEmailAddress(pattern)
}

/** Does an address fall inside a pattern? Concrete patterns compare
 *  case-insensitively; catch-alls match on the domain suffix. */
export function patternMatchesEmail(
  pattern: EmailPattern,
  email: string,
): boolean {
  const normalizedEmail = email.trim().toLowerCase()
  const concrete = patternEmailAddress(pattern)
  if (concrete) {
    return concrete.toLowerCase() === normalizedEmail
  }
  return normalizedEmail.endsWith(pattern.toLowerCase().slice(1))
}

export interface AddressSuggestionOption {
  name: string | null
  email: string
  sourceLabel: string
  origin: 'account' | 'correspondent'
}

/** The slice of a `senderAddresses` row the suggestion builder consumes. */
export interface AddressBookEntry {
  name: string | null
  email: string
}

export function formatAddressSuggestion(
  option: AddressSuggestionOption,
): string {
  return option.name ? `${option.name} <${option.email}>` : option.email
}

/**
 * Map the persistent server-side address book (`senderAddresses` view) into the
 * same suggestion options the compose recipient inputs consume, so the rules
 * editor's address fields share one autocomplete engine with compose. Filters
 * non-concrete addresses and de-dupes by lowercased email.
 */
export function buildAddressBookSuggestionOptions(
  addresses: readonly AddressBookEntry[],
): AddressSuggestionOption[] {
  const seen = new Set<string>()
  const options: AddressSuggestionOption[] = []
  for (const address of addresses) {
    const email = parseEmailAddress(address.email)
    if (!email) {
      continue
    }
    const key = email.toLowerCase()
    if (seen.has(key)) {
      continue
    }
    seen.add(key)
    options.push({
      name: address.name,
      email,
      sourceLabel: 'Address book',
      origin: 'correspondent',
    })
  }
  return options
}

/**
 * Case-insensitive SUBSTRING filter over name, email, source label, and the
 * combined "Name <email>" rendering — never a prefix-only match.
 *
 * `mode` controls what the needle is:
 * * `'token'` (compose recipient fields): only the text after the last
 *   comma/semicolon — the address currently being typed in a list.
 * * `'whole'` (single-value inputs, e.g. a rule condition): the ENTIRE input.
 *   A single-value field must never be comma-tokenized — a value like
 *   `"Doe, John"` would otherwise silently filter on `"John"` only.
 */
export function filterAddressSuggestions(
  options: AddressSuggestionOption[],
  value: string,
  limit = 8,
  mode: 'token' | 'whole' = 'token',
): AddressSuggestionOption[] {
  const needle = (
    mode === 'token' ? currentAddressToken(value) : value.trim()
  ).toLowerCase()
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
