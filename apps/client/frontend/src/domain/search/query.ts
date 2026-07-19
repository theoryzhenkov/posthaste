import { parseIsoDate } from '../time'
import { IS_VALUES } from './definitions'
import { normalize, prefixDefinition } from './scan'
import { parseQueryTokens } from './parser'
import type { QueryValidation } from './types'

function validatePrefixedValue(prefix: string, value: string): QueryValidation {
  if (!value.trim()) {
    return { state: 'incomplete', message: `empty value for ${prefix}:` }
  }

  const normalizedPrefix = prefixDefinition(prefix)?.primary
  const normalizedValue = normalize(value)
  switch (normalizedPrefix) {
    case 'is':
      return IS_VALUES.includes(normalizedValue)
        ? { state: 'valid' }
        : { state: 'invalid', message: `unknown is: value: ${value}` }
    case 'has':
      return normalizedValue === 'attachment' ||
        normalizedValue === 'attachments'
        ? { state: 'valid' }
        : { state: 'invalid', message: `unknown has: value: ${value}` }
    case 'date':
      return parseIsoDate(normalizedValue)
        ? { state: 'valid' }
        : { state: 'invalid', message: `invalid date '${value}'` }
    case 'newer':
    case 'older':
      return /^\d+[dwmy]$/.test(normalizedValue)
        ? { state: 'valid' }
        : {
            state: 'invalid',
            message: `invalid relative date '${value}', expected e.g. 2w`,
          }
    default:
      return { state: 'valid' }
  }
}

export function validateSearchQuery(input: string): QueryValidation {
  const query = input.trim()
  if (!query) {
    return { state: 'incomplete', message: 'empty query' }
  }

  const parsed = parseQueryTokens(query)
  if (!parsed.ok) {
    return parsed.validation
  }

  for (const token of parsed.tokens) {
    if (!token.prefix) {
      continue
    }
    const validation = validatePrefixedValue(token.prefix, token.value)
    if (validation.state !== 'valid') {
      return validation
    }
  }

  return { state: 'valid' }
}


/** A normalized query the SERVER search accepts: whitespace-collapsed and
 *  validated against the prefix grammar. Empty means "no filter". */
export type SearchQuery = string & { readonly __brand: 'SearchQuery' }

export interface PreparedServerSearchQuery {
  query: SearchQuery | undefined
  validation: QueryValidation
  isBlocked: boolean
}

export function normalizeAppliedSearchQuery(value: string): string {
  return value.trim().replace(/\s+/g, ' ')
}

/**
 * Build the query that filters a view to a single message's conversation. The
 * backend search supports the `conversation:` prefix (matching
 * `MailQueryField::ConversationId`); conversation ids are whitespace-free
 * tokens, so no quoting is needed — the result is valid by construction.
 */
export function conversationViewQuery(conversationId: string): SearchQuery {
  return `conversation:${conversationId}` as SearchQuery
}

/** Parse raw search text into an applicable query, or `null` when the grammar
 *  rejects it. Empty/whitespace input parses to the empty query (clear). */
export function parseSearchQuery(value: string): SearchQuery | null {
  const prepared = prepareServerSearchQuery(value)
  if (prepared.isBlocked) {
    return null
  }
  return prepared.query ?? ('' as SearchQuery)
}

export function prepareServerSearchQuery(
  value: string | undefined,
): PreparedServerSearchQuery {
  const normalized = normalizeAppliedSearchQuery(value ?? '')
  if (!normalized) {
    return {
      query: undefined,
      validation: { state: 'valid' },
      isBlocked: false,
    }
  }

  const validation = validateSearchQuery(normalized)
  if (validation.state !== 'valid') {
    return {
      query: undefined,
      validation,
      isBlocked: true,
    }
  }

  return {
    query: normalized as SearchQuery,
    validation,
    isBlocked: false,
  }
}
