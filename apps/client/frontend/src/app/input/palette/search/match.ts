import type { MatchEvidence } from './types'

type QueryMatchKind =
  | 'exact'
  | 'prefix'
  | 'acronym'
  | 'contains'
  | 'fuzzy'
  | 'fts'
  | 'none'

export interface TextMatchResult {
  kind: QueryMatchKind
  score: number
}

function normalizeSearchText(value: string): string {
  return value.trim().toLowerCase()
}

function acronym(value: string): string {
  return value
    .split(/[^a-zA-Z0-9]+/)
    .filter(Boolean)
    .map((part) => part[0]?.toLowerCase() ?? '')
    .join('')
}

function fuzzyIncludes(query: string, value: string): boolean {
  let queryIndex = 0
  for (const char of value) {
    if (char === query[queryIndex]) {
      queryIndex += 1
      if (queryIndex === query.length) {
        return true
      }
    }
  }
  return query.length === 0
}

export function textMatch(
  query: string,
  primary: string,
  haystack = '',
): TextMatchResult {
  const normalizedQuery = normalizeSearchText(query)
  if (!normalizedQuery) {
    return { kind: 'none', score: 0 }
  }

  const normalizedPrimary = normalizeSearchText(primary)
  const normalizedHaystack = normalizeSearchText(`${primary} ${haystack}`)
  if (normalizedPrimary === normalizedQuery) {
    return { kind: 'exact', score: 100 }
  }
  if (normalizedPrimary.startsWith(normalizedQuery)) {
    return { kind: 'prefix', score: 90 }
  }
  if (acronym(primary).startsWith(normalizedQuery)) {
    return { kind: 'acronym', score: 75 }
  }
  if (normalizedHaystack.includes(normalizedQuery)) {
    return { kind: 'contains', score: 60 }
  }
  if (
    normalizedQuery.length > 1 &&
    fuzzyIncludes(normalizedQuery, normalizedHaystack)
  ) {
    return { kind: 'fuzzy', score: 40 }
  }
  return { kind: 'none', score: 0 }
}

export function matchesQuery(
  query: string,
  primary: string,
  haystack = '',
): boolean {
  return (
    !normalizeSearchText(query) ||
    textMatch(query, primary, haystack).kind !== 'none'
  )
}

export function matchEvidence(
  query: string,
  field: MatchEvidence['fields'][number]['field'],
  match: TextMatchResult,
): MatchEvidence {
  return {
    query,
    fields:
      match.kind === 'none'
        ? []
        : [
            {
              field,
              kind: match.kind,
            },
          ],
  }
}
