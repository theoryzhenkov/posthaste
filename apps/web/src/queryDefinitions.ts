// Static search-query vocabulary: prefix definitions, value tables, and help entries.
// Split out of queryLanguage.ts so the tokenizer/validation/completion logic reads as behavior,
// not data. Pure data + a single help-entry builder; no runtime dependencies.

export interface QueryPrefixDefinition {
  primary: string
  aliases: string[]
  label: string
  description: string
  valueHint: string
}

export interface QueryHelpEntry {
  id: string
  label: string
  detail: string
  replacement: string
  keywords: string
}

export const PREFIX_DEFINITIONS = [
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

export const PREFIX_BY_NAME = new Map<string, QueryPrefixDefinition>(
  PREFIX_DEFINITIONS.flatMap((definition) => [
    [definition.primary, definition],
    ...definition.aliases.map((alias) => [alias, definition] as const),
  ]),
)

export const SPACED_VALUE_PREFIXES = new Set([
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

export const IS_VALUES = [
  'unread',
  'read',
  'seen',
  'flagged',
  'unflagged',
  'attachment',
]
export const RELATIVE_DATE_VALUES = ['1d', '2d', '1w', '2w', '1m', '1y']

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

export const HELP_ENTRIES: QueryHelpEntry[] = [
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
