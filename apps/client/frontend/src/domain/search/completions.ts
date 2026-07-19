import { now } from '@/lib/ambient/time'
import { KNOWN_MAILBOX_ROLES } from '../vocabulary'
import {
  IS_VALUES,
  PREFIX_BY_NAME,
  PREFIX_DEFINITIONS,
  RELATIVE_DATE_VALUES,
  SPACED_VALUE_PREFIXES,
} from './definitions'
import { todayIsoDate } from '../time'
import {
  PREFIX_CHAR,
  WHITESPACE,
  activeBareToken,
  filterCandidates,
  normalize,
  uniqueCandidates,
  userTagCandidate,
} from './scan'
import type {
  QueryCompletion,
  QueryCompletionContext,
  ValueCandidate,
} from './types'

export function findActivePrefix(input: string): {
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
    if (index > 0 && !WHITESPACE.test(input[index - 1] ?? '')) {
      continue
    }

    let nameStart = index
    if (input[nameStart] === '-') {
      nameStart += 1
    }

    let nameEnd = nameStart
    while (nameEnd < input.length && PREFIX_CHAR.test(input[nameEnd] ?? '')) {
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
    while (valueStart < input.length && WHITESPACE.test(input[valueStart] ?? '')) {
      valueStart += 1
    }

    if (!acceptsSpacedValue) {
      let valueEnd = valueStart
      while (valueEnd < input.length && !WHITESPACE.test(input[valueEnd] ?? '')) {
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
        ...context.sources.flatMap((source) =>
          source.mailboxes.map((mailbox) => ({
            value: mailbox.name,
            label: mailbox.name,
            detail: source.name,
            keywords: `${mailbox.id} ${mailbox.role ?? ''}`,
          })),
        ),
        ...KNOWN_MAILBOX_ROLES.map((role) => ({
          value: role,
          label: role,
          detail: 'Mailbox role',
        })),
      ]
    case 'source':
      return context.sources.map((source) => ({
        value: source.name,
        label: source.name,
        detail: 'Account',
        keywords: source.id,
      }))
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
        ...context.tags.flatMap((tag) => {
          const candidate = userTagCandidate(tag.name, 'Tag')
          return candidate ? [candidate] : []
        }),
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
      const today = todayIsoDate(context.now ?? now())
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
