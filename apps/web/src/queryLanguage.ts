import type { MessageSummary, SidebarResponse } from './api/types'
import { KNOWN_MAILBOX_ROLES, SYSTEM_KEYWORD_PREFIX } from './domainVocabulary'
import {
  HELP_ENTRIES,
  IS_VALUES,
  PREFIX_BY_NAME,
  PREFIX_DEFINITIONS,
  RELATIVE_DATE_VALUES,
  SPACED_VALUE_PREFIXES,
  type QueryHelpEntry,
  type QueryPrefixDefinition,
} from './queryDefinitions'

export type { QueryHelpEntry, QueryPrefixDefinition }

export interface QueryCompletion {
  id: string
  label: string
  replacement: string
  detail: string
  kind: 'prefix' | 'value'
}

export type QueryValidation =
  | { state: 'valid' }
  | { state: 'incomplete'; message: string }
  | { state: 'invalid'; message: string }

interface QueryCompletionContext {
  sidebar: SidebarResponse | undefined
  messages: MessageSummary[]
  now?: Date
}

interface ValueCandidate {
  value: string
  label: string
  detail: string
  keywords?: string
}

function isWhitespace(value: string): boolean {
  return /\s/.test(value)
}

function isPrefixChar(value: string): boolean {
  return /[a-zA-Z]/.test(value)
}

function normalize(value: string): string {
  return value.trim().toLowerCase()
}

function prefixDefinition(prefix: string): QueryPrefixDefinition | undefined {
  return PREFIX_BY_NAME.get(prefix.toLowerCase())
}

function todayIsoDate(now: Date): string {
  return now.toISOString().slice(0, 10)
}

function uniqueCandidates(candidates: ValueCandidate[]): ValueCandidate[] {
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

function userTagCandidate(
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

function filterCandidates(
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

function activeBareToken(input: string): { start: number; value: string } {
  let start = input.length
  while (start > 0 && !isWhitespace(input[start - 1] ?? '')) {
    start -= 1
  }
  return { start, value: input.slice(start) }
}

function parseQueryTokens(input: string):
  | {
      ok: true
      tokens: Array<{ prefix: string | null; value: string }>
    }
  | {
      ok: false
      validation: QueryValidation
    } {
  const tokens: Array<{ prefix: string | null; value: string }> = []
  const chars = [...input]
  let index = 0

  while (index < chars.length) {
    while (index < chars.length && isWhitespace(chars[index] ?? '')) {
      index += 1
    }
    if (index >= chars.length) {
      break
    }

    if (
      chars[index] === '-' &&
      index + 1 < chars.length &&
      !isWhitespace(chars[index + 1] ?? '')
    ) {
      index += 1
    }

    const start = index
    let colonIndex: number | null = null
    while (index < chars.length && !isWhitespace(chars[index] ?? '')) {
      if (chars[index] === ':') {
        colonIndex = index
        break
      }
      index += 1
    }

    if (colonIndex === null) {
      index = start
      tokens.push({ prefix: null, value: scanTokenValue(chars, index).value })
      index = scanTokenValue(chars, index).end
      continue
    }

    const prefix = input.slice(start, colonIndex).toLowerCase()
    const definition = prefixDefinition(prefix)
    if (!definition) {
      return {
        ok: false,
        validation: {
          state: 'invalid',
          message: `unknown search prefix: ${prefix}`,
        },
      }
    }

    index = colonIndex + 1
    while (index < chars.length && isWhitespace(chars[index] ?? '')) {
      index += 1
    }

    let value: string
    if (SPACED_VALUE_PREFIXES.has(prefix)) {
      if (chars[index] === '"') {
        const scanned = scanTokenValue(chars, index)
        value = scanned.value
        index = scanned.end
      } else if (startsKnownPrefixTokenAt(chars, index)) {
        value = ''
      } else {
        const valueStart = index
        while (index < chars.length) {
          if (startsKnownPrefixAt(chars, index)) {
            break
          }
          index += 1
        }
        value = input.slice(valueStart, index).trim()
      }
    } else {
      const scanned = scanTokenValue(chars, index)
      value = scanned.value
      index = scanned.end
    }

    tokens.push({ prefix, value })
  }

  return { ok: true, tokens }
}

function scanTokenValue(
  chars: string[],
  start: number,
): { value: string; end: number } {
  if (chars[start] === '"') {
    let end = start + 1
    while (end < chars.length && chars[end] !== '"') {
      end += 1
    }
    const value = chars.slice(start + 1, end).join('')
    return { value, end: end < chars.length ? end + 1 : end }
  }

  let end = start
  while (end < chars.length && !isWhitespace(chars[end] ?? '')) {
    end += 1
  }
  return { value: chars.slice(start, end).join(''), end }
}

function startsKnownPrefixTokenAt(chars: string[], position: number): boolean {
  if (position >= chars.length) {
    return false
  }

  let index = position
  if (chars[index] === '-') {
    index += 1
  }

  const start = index
  while (index < chars.length && !isWhitespace(chars[index] ?? '')) {
    if (chars[index] === ':') {
      return prefixDefinition(chars.slice(start, index).join('')) !== undefined
    }
    index += 1
  }
  return false
}

function startsKnownPrefixAt(chars: string[], position: number): boolean {
  if (position >= chars.length || !isWhitespace(chars[position] ?? '')) {
    return false
  }

  let index = position
  while (index < chars.length && isWhitespace(chars[index] ?? '')) {
    index += 1
  }
  if (chars[index] === '-') {
    index += 1
  }

  const start = index
  while (index < chars.length && !isWhitespace(chars[index] ?? '')) {
    if (chars[index] === ':') {
      return prefixDefinition(chars.slice(start, index).join('')) !== undefined
    }
    index += 1
  }
  return false
}

function isValidIsoDate(value: string): boolean {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) {
    return false
  }
  const date = new Date(`${value}T00:00:00.000Z`)
  return !Number.isNaN(date.getTime()) && date.toISOString().startsWith(value)
}

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
      return isValidIsoDate(normalizedValue)
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

function findActivePrefix(input: string): {
  name: string
  valueStart: number
  value: string
} | null {
  let active: {
    name: string
    valueStart: number
    value: string
  } | null = null

  for (let index = 0; index < input.length; index += 1) {
    if (index > 0 && !isWhitespace(input[index - 1] ?? '')) {
      continue
    }

    let nameStart = index
    if (input[nameStart] === '-') {
      nameStart += 1
    }

    let nameEnd = nameStart
    while (nameEnd < input.length && isPrefixChar(input[nameEnd] ?? '')) {
      nameEnd += 1
    }

    if (input[nameEnd] !== ':') {
      continue
    }

    const name = input.slice(nameStart, nameEnd).toLowerCase()
    if (!PREFIX_BY_NAME.has(name)) {
      continue
    }

    const acceptsSpacedValue = SPACED_VALUE_PREFIXES.has(name)
    let valueStart = nameEnd + 1
    while (valueStart < input.length && isWhitespace(input[valueStart] ?? '')) {
      valueStart += 1
    }

    if (!acceptsSpacedValue) {
      let valueEnd = valueStart
      while (valueEnd < input.length && !isWhitespace(input[valueEnd] ?? '')) {
        valueEnd += 1
      }
      if (valueEnd < input.length) {
        continue
      }
      active = {
        name,
        valueStart,
        value: input.slice(valueStart, valueEnd),
      }
      continue
    }

    active = {
      name,
      valueStart,
      value: input.slice(valueStart),
    }
  }

  return active
}

function prefixSuggestions(input: string): QueryCompletion[] {
  const token = activeBareToken(input)
  const fragment = normalize(token.value.replace(/^-/, ''))
  if (token.value.includes(':')) {
    return []
  }

  return PREFIX_DEFINITIONS.filter((definition) => {
    if (!fragment) {
      return ['from', 'subject', 'in', 'is', 'has', 'newer'].includes(
        definition.primary,
      )
    }
    const names = [definition.primary, ...definition.aliases, definition.label]
    return names.some((name) => name.startsWith(fragment))
  })
    .slice(0, 8)
    .map((definition) => ({
      id: `prefix:${definition.primary}`,
      kind: 'prefix',
      label: definition.label,
      detail: `${definition.description} - ${definition.valueHint}`,
      replacement: `${input.slice(0, token.start)}${definition.primary}:`,
    }))
}

function candidatesForPrefix(
  prefix: string,
  context: QueryCompletionContext,
): ValueCandidate[] {
  const definition = PREFIX_BY_NAME.get(prefix)
  if (!definition) {
    return []
  }

  switch (definition.primary) {
    case 'in':
      return [
        ...(context.sidebar?.sources.flatMap((source) =>
          source.mailboxes.map((mailbox) => ({
            value: mailbox.name,
            label: mailbox.name,
            detail: source.name,
            keywords: `${mailbox.id} ${mailbox.role ?? ''}`,
          })),
        ) ?? []),
        ...KNOWN_MAILBOX_ROLES.map((role) => ({
          value: role,
          label: role,
          detail: 'Mailbox role',
        })),
      ]
    case 'source':
      return (
        context.sidebar?.sources.map((source) => ({
          value: source.name,
          label: source.name,
          detail: 'Account',
          keywords: source.id,
        })) ?? []
      )
    case 'is':
      return IS_VALUES.map((value) => ({
        value,
        label: value,
        detail: 'Message state',
      }))
    case 'has':
      return [
        { value: 'attachment', label: 'attachment', detail: 'Message has' },
      ]
    case 'tag':
      return uniqueCandidates([
        ...(context.sidebar?.tags.flatMap((tag) => {
          const candidate = userTagCandidate(tag.name, 'Tag')
          return candidate ? [candidate] : []
        }) ?? []),
        ...context.messages.flatMap((message) =>
          message.keywords
            .map((keyword) => userTagCandidate(keyword, 'Keyword'))
            .filter((candidate): candidate is ValueCandidate =>
              Boolean(candidate),
            ),
        ),
      ])
    case 'from':
      return uniqueCandidates(
        context.messages.flatMap((message) => {
          const candidates: ValueCandidate[] = []
          if (message.fromName) {
            candidates.push({
              value: message.fromName,
              label: message.fromName,
              detail: message.fromEmail ?? 'Sender',
            })
          }
          if (message.fromEmail) {
            candidates.push({
              value: message.fromEmail,
              label: message.fromEmail,
              detail: message.fromName ?? 'Sender',
            })
          }
          return candidates
        }),
      )
    case 'newer':
    case 'older':
      return RELATIVE_DATE_VALUES.map((value) => ({
        value,
        label: value,
        detail: 'Relative date',
      }))
    case 'before':
    case 'after':
    case 'date': {
      const today = todayIsoDate(context.now ?? new Date())
      return [{ value: today, label: today, detail: 'Today' }]
    }
    default:
      return []
  }
}

function valueSuggestions(
  input: string,
  context: QueryCompletionContext,
): QueryCompletion[] {
  const activePrefix = findActivePrefix(input)
  if (!activePrefix) {
    return []
  }

  const candidates = filterCandidates(
    candidatesForPrefix(activePrefix.name, context),
    activePrefix.value,
  )

  return candidates.map((candidate) => ({
    id: `value:${activePrefix.name}:${candidate.value}`,
    kind: 'value',
    label: candidate.label,
    detail: candidate.detail,
    replacement: `${input.slice(0, activePrefix.valueStart)}${candidate.value}`,
  }))
}

export function getQueryCompletions(
  input: string,
  context: QueryCompletionContext,
): QueryCompletion[] {
  const activePrefix = findActivePrefix(input)
  if (activePrefix) {
    return valueSuggestions(input, context)
  }
  return prefixSuggestions(input)
}

export function getQueryHelpEntries(input: string): QueryHelpEntry[] {
  const normalized = normalize(input)
  if (!normalized || normalized === '?' || normalized === 'help') {
    return HELP_ENTRIES.slice(0, 8)
  }

  const helpMode =
    normalized.includes('help') ||
    normalized.includes('query') ||
    normalized.includes('filter')

  if (!helpMode) {
    const activePrefix = findActivePrefix(input)
    if (!activePrefix) {
      return []
    }
    const definition = PREFIX_BY_NAME.get(activePrefix.name)
    return HELP_ENTRIES.filter((entry) =>
      definition
        ? entry.label === `${definition.primary}:`
        : entry.keywords.includes(activePrefix.name),
    )
  }

  const terms = normalized
    .split(/\s+/)
    .filter((term) => !['help', 'query', 'filter', 'search'].includes(term))
  if (terms.length === 0) {
    return HELP_ENTRIES
  }
  return HELP_ENTRIES.filter((entry) =>
    terms.every((term) => entry.keywords.includes(term)),
  )
}
