import { SYSTEM_KEYWORD_PREFIX } from '../domainVocabulary'
import { PREFIX_BY_NAME, type QueryPrefixDefinition } from '../queryDefinitions'
import type { ValueCandidate } from './types'

export function isWhitespace(value: string): boolean {
  return /\s/.test(value)
}

export function isPrefixChar(value: string): boolean {
  return /[a-zA-Z]/.test(value)
}

export function normalize(value: string): string {
  return value.trim().toLowerCase()
}

export function prefixDefinition(
  prefix: string,
): QueryPrefixDefinition | undefined {
  return PREFIX_BY_NAME.get(prefix.toLowerCase())
}

export function todayIsoDate(now: Date): string {
  return now.toISOString().slice(0, 10)
}

export function uniqueCandidates(
  candidates: ValueCandidate[],
): ValueCandidate[] {
  const seen = new Set<string>()
  const unique: ValueCandidate[] = []
  for (const candidate of candidates) {
    const key = candidate.value.toLowerCase()
    if (seen.has(key)) {
      continue
    }
    seen.add(key)
    unique.push(candidate)
  }
  return unique
}

export function userTagCandidate(
  value: string,
  detail: string,
): ValueCandidate | null {
  const tag = value.trim()
  if (!tag || tag.startsWith(SYSTEM_KEYWORD_PREFIX)) {
    return null
  }
  return {
    value: tag,
    label: tag,
    detail,
  }
}

export function filterCandidates(
  candidates: ValueCandidate[],
  valueFragment: string,
): ValueCandidate[] {
  const fragment = normalize(valueFragment)
  const filtered = uniqueCandidates(candidates)
    .map((candidate, index) => ({ candidate, index }))
    .filter(({ candidate }) => {
      if (!fragment) {
        return true
      }
      const haystack =
        `${candidate.value} ${candidate.label} ${candidate.detail} ${candidate.keywords ?? ''}`.toLowerCase()
      return haystack.includes(fragment)
    })

  return filtered
    .sort((left, right) => {
      const leftStarts = left.candidate.value.toLowerCase().startsWith(fragment)
      const rightStarts = right.candidate.value
        .toLowerCase()
        .startsWith(fragment)
      if (leftStarts !== rightStarts) {
        return leftStarts ? -1 : 1
      }
      return left.index - right.index
    })
    .map(({ candidate }) => candidate)
    .slice(0, 8)
}

export function activeBareToken(input: string): {
  start: number
  value: string
} {
  let start = input.length
  while (start > 0 && !isWhitespace(input[start - 1] ?? '')) {
    start -= 1
  }
  return { start, value: input.slice(start) }
}
