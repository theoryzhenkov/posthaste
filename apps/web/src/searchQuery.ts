import type { MessageSummary } from './api/types'

export interface ParsedSearchQuery {
  textTerms: string[]
  fromTerms: string[]
}

const modifierPattern = /(^|\s)(f|from):/gi

function normalizeTerm(value: string): string {
  return value.trim().replace(/\s+/g, ' ').toLowerCase()
}

function splitFreeText(value: string): string[] {
  return value
    .trim()
    .split(/\s+/)
    .map((term) => term.toLowerCase())
    .filter(Boolean)
}

export function normalizeAppliedSearchQuery(value: string): string {
  return value.trim().replace(/\s+/g, ' ')
}

export function parseSearchQuery(value: string): ParsedSearchQuery {
  const textTerms: string[] = []
  const fromTerms: string[] = []
  const matches = [...value.matchAll(modifierPattern)]

  if (matches.length === 0) {
    return { textTerms: splitFreeText(value), fromTerms }
  }

  let cursor = 0
  for (let index = 0; index < matches.length; index += 1) {
    const match = matches[index]
    const prefixStart = match.index + match[1].length
    const valueStart = match.index + match[0].length
    const nextPrefixStart =
      index + 1 < matches.length ? matches[index + 1].index : value.length

    textTerms.push(...splitFreeText(value.slice(cursor, prefixStart)))

    const modifierValue = normalizeTerm(
      value.slice(valueStart, nextPrefixStart),
    )
    if (modifierValue) {
      fromTerms.push(modifierValue)
    }

    cursor = nextPrefixStart
  }

  textTerms.push(...splitFreeText(value.slice(cursor)))
  return { textTerms, fromTerms }
}

function includesAllTerms(haystack: string, terms: string[]): boolean {
  return terms.every((term) => haystack.includes(term))
}

function buildHaystack(parts: Array<string | null | undefined>): string {
  return parts.filter(Boolean).join(' ').toLowerCase()
}

export function matchesMessageSearch(
  message: MessageSummary,
  query?: string,
): boolean {
  if (!query?.trim()) {
    return true
  }

  const parsed = parseSearchQuery(query)
  const textHaystack = buildHaystack([
    message.subject,
    message.preview,
    message.fromName,
    message.fromEmail,
    message.sourceName,
    message.keywords.join(' '),
  ])
  const fromHaystack = buildHaystack([message.fromName, message.fromEmail])

  return (
    includesAllTerms(textHaystack, parsed.textTerms) &&
    includesAllTerms(fromHaystack, parsed.fromTerms)
  )
}
