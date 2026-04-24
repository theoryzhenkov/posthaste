import { describe, expect, it } from 'bun:test'

import type {
  AutomationRule,
  SmartMailbox,
  SmartMailboxRule,
} from '../src/api/types'
import {
  actionConditionFromSmartMailboxRule,
  draftToRule,
  extractAccountIdFromRule,
  ruleToDraft,
  smartMailboxDraftToRule,
  smartMailboxRulePrefix,
} from '../src/automationRules'

const actionRule: SmartMailboxRule = {
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

const smartMailboxRule: SmartMailboxRule = {
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

const smartMailbox: SmartMailbox = {
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

describe('automation rule projection', () => {
  it('serializes an account draft as a global source-scoped automation rule', () => {
    const rule = draftToRule({
      id: ' rule-1 ',
      accountId: 'primary',
      name: ' Newsletters ',
      enabled: true,
      triggers: [],
      condition: actionRule,
      actions: [{ kind: 'applyTag', tag: ' newsletter ' }],
      backfill: true,
    })

    expect(rule).toMatchObject({
      id: 'rule-1',
      name: 'Newsletters',
      triggers: ['messageArrived'],
      actions: [{ kind: 'applyTag', tag: 'newsletter' }],
      condition: {
        root: {
          operator: 'all',
          nodes: [
            {
              type: 'condition',
              field: 'sourceId',
              operator: 'equals',
              value: 'primary',
            },
            {
              type: 'group',
              operator: 'all',
              nodes: actionRule.root.nodes,
            },
          ],
        },
      },
    })
  })

  it('hydrates an account-scoped saved rule back to editable action conditions', () => {
    const saved = draftToRule({
      id: 'rule-1',
      accountId: 'primary',
      name: 'Newsletters',
      enabled: true,
      triggers: ['messageArrived'],
      condition: actionRule,
      actions: [{ kind: 'applyTag', tag: 'newsletter' }],
      backfill: true,
    })

    expect(ruleToDraft('primary', saved)).toMatchObject({
      id: 'rule-1',
      accountId: 'primary',
      condition: actionRule,
    })
  })

  it('serializes smart mailbox actions as source and smart mailbox constrained rules', () => {
    const rule = smartMailboxDraftToRule(
      {
        id: `${smartMailboxRulePrefix(smartMailbox.id)}rule-1`,
        accountId: 'primary',
        name: 'Archive action',
        enabled: true,
        triggers: ['messageArrived'],
        condition: actionRule,
        actions: [{ kind: 'markRead' }],
        backfill: true,
      },
      smartMailbox,
    )

    expect(rule.condition.root.nodes).toEqual([
      {
        type: 'condition',
        field: 'sourceId',
        operator: 'equals',
        negated: false,
        value: 'primary',
      },
      {
        type: 'group',
        operator: 'all',
        negated: false,
        nodes: [
          {
            type: 'group',
            operator: 'all',
            negated: false,
            nodes: smartMailboxRule.root.nodes,
          },
          {
            type: 'group',
            operator: 'all',
            negated: false,
            nodes: actionRule.root.nodes,
          },
        ],
      },
    ])
  })

  it('hydrates smart mailbox action rules using the source condition and inner action condition', () => {
    const saved: AutomationRule = smartMailboxDraftToRule(
      {
        id: `${smartMailboxRulePrefix(smartMailbox.id)}rule-1`,
        accountId: 'primary',
        name: 'Archive action',
        enabled: true,
        triggers: ['messageArrived'],
        condition: actionRule,
        actions: [{ kind: 'markRead' }],
        backfill: true,
      },
      smartMailbox,
    )

    expect(extractAccountIdFromRule(saved, 'fallback')).toBe('primary')
    expect(actionConditionFromSmartMailboxRule(saved, 'primary')).toEqual(
      actionRule,
    )
  })
})
