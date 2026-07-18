import { validateSearchQuery, type QueryValidation } from './search/index'

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
