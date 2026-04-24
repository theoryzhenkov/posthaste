import type { MessageSummary, SidebarResponse } from './api/types'

export interface QueryPrefixDefinition {
  primary: string
  aliases: string[]
  label: string
  description: string
  valueHint: string
}

export interface QueryCompletion {
  id: string
  label: string
  replacement: string
  detail: string
  kind: 'prefix' | 'value'
}

export interface QueryHelpEntry {
  id: string
  label: string
  detail: string
  replacement: string
  keywords: string
}

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

const PREFIX_DEFINITIONS = [
  {
    primary: 'from',
    aliases: ['f', 'sender'],
    label: 'from:',
    description: 'Sender name or email',
    valueHint: 'person@example.com',
  },
  {
    primary: 'subject',
    aliases: ['s'],
    label: 'subject:',
    description: 'Subject text',
    valueHint: 'account creation',
  },
  {
    primary: 'body',
    aliases: ['preview'],
    label: 'body:',
    description: 'Synced preview text',
    valueHint: 'receipt',
  },
  {
    primary: 'in',
    aliases: ['mailbox'],
    label: 'in:',
    description: 'Mailbox name, role, or ID',
    valueHint: 'Archive',
  },
  {
    primary: 'source',
    aliases: ['account'],
    label: 'source:',
    description: 'Account name or ID',
    valueHint: 'Personal',
  },
  {
    primary: 'is',
    aliases: [],
    label: 'is:',
    description: 'Message state',
    valueHint: 'unread',
  },
  {
    primary: 'has',
    aliases: [],
    label: 'has:',
    description: 'Message capability',
    valueHint: 'attachment',
  },
  {
    primary: 'tag',
    aliases: ['keyword'],
    label: 'tag:',
    description: 'JMAP keyword or label',
    valueHint: '$label',
  },
  {
    primary: 'newer',
    aliases: [],
    label: 'newer:',
    description: 'Relative lower date bound',
    valueHint: '2w',
  },
  {
    primary: 'older',
    aliases: [],
    label: 'older:',
    description: 'Relative upper date bound',
    valueHint: '1y',
  },
  {
    primary: 'before',
    aliases: [],
    label: 'before:',
    description: 'Exclusive date upper bound',
    valueHint: '2026-04-24',
  },
  {
    primary: 'after',
    aliases: [],
    label: 'after:',
    description: 'Inclusive date lower bound',
    valueHint: '2026-04-24',
  },
  {
    primary: 'date',
    aliases: [],
    label: 'date:',
    description: 'Single calendar date',
    valueHint: '2026-04-24',
  },
  {
    primary: 'id',
    aliases: [],
    label: 'id:',
    description: 'Exact message ID',
    valueHint: 'message-id',
  },
  {
    primary: 'thread',
    aliases: ['threadid'],
    label: 'thread:',
    description: 'Exact thread ID',
    valueHint: 'thread-id',
  },
] as const satisfies readonly QueryPrefixDefinition[]

const PREFIX_BY_NAME = new Map<string, QueryPrefixDefinition>(
  PREFIX_DEFINITIONS.flatMap((definition) => [
    [definition.primary, definition],
    ...definition.aliases.map((alias) => [alias, definition] as const),
  ]),
)

const SPACED_VALUE_PREFIXES = new Set([
  'from',
  'f',
  'sender',
  'subject',
  's',
  'body',
  'preview',
  'tag',
  'keyword',
  'in',
  'mailbox',
  'source',
  'account',
])

const MAILBOX_ROLES = ['inbox', 'archive', 'drafts', 'sent', 'junk', 'trash']
const IS_VALUES = [
  'unread',
  'read',
  'seen',
  'flagged',
  'unflagged',
  'attachment',
]
const RELATIVE_DATE_VALUES = ['1d', '2d', '1w', '2w', '1m', '1y']

const HELP_ENTRIES: QueryHelpEntry[] = [
  help('from:', 'Sender name or email', 'from: ', 'f sender person email'),
  help('subject:', 'Subject text, spaces allowed', 'subject: ', 's title'),
  help('in:', 'Mailbox name, role, or ID', 'in: ', 'mailbox folder archive'),
  help('is:', 'read, unread, flagged, unflagged', 'is:', 'state status seen'),
  help('has:', 'attachment', 'has:', 'attachment'),
  help('tag:', 'JMAP keyword or label', 'tag:', 'keyword label'),
  help('source:', 'Account name or ID', 'source: ', 'account source'),
  help('newer:', 'Relative age such as 2w', 'newer:', 'after recent date'),
  help('older:', 'Relative age such as 1y', 'older:', 'before old date'),
  help('date:', 'Exact YYYY-MM-DD date', 'date:', 'calendar day'),
  help('id:', 'Exact message ID', 'id:', 'message id'),
  help('thread:', 'Exact thread ID', 'thread:', 'thread id'),
]

function help(
  label: string,
  detail: string,
  replacement: string,
  keywords: string,
): QueryHelpEntry {
  return {
    id: `help:${label}`,
    label,
    detail,
    replacement,
    keywords: `${label} ${detail} ${keywords}`.toLowerCase(),
  }
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
        ...MAILBOX_ROLES.map((role) => ({
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
      return uniqueCandidates(
        context.messages.flatMap((message) =>
          message.keywords
            .filter((keyword) => !keyword.startsWith('$'))
            .map((keyword) => ({
              value: keyword,
              label: keyword,
              detail: 'Keyword',
            })),
        ),
      )
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
