import type { SmartMailbox, SmartMailboxRule } from '../src/api/types'

export const actionRule: SmartMailboxRule = {
  root: {
    operator: 'all',
    negated: false,
    nodes: [
      {
        type: 'condition',
        field: 'fromName',
        operator: 'contains',
        negated: false,
        value: 'Posthaste',
      },
    ],
  },
}

export const smartMailboxRule: SmartMailboxRule = {
  root: {
    operator: 'all',
    negated: false,
    nodes: [
      {
        type: 'condition',
        field: 'mailboxRole',
        operator: 'equals',
        negated: false,
        value: 'archive',
      },
    ],
  },
}

export const smartMailbox: SmartMailbox = {
  id: 'smart-archive',
  name: 'Archive',
  position: 0,
  kind: 'user',
  defaultKey: null,
  parentId: null,
  rule: smartMailboxRule,
  createdAt: '2026-04-24T00:00:00Z',
  updatedAt: '2026-04-24T00:00:00Z',
}
