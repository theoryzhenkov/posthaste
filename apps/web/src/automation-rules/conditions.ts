import type {
  AutomationRule,
  MailQueryGroup,
  MailQueryRule,
} from '../api/types'

export function cloneRule(rule: MailQueryRule): MailQueryRule {
  return JSON.parse(JSON.stringify(rule)) as MailQueryRule
}

export function groupNode(group: MailQueryGroup) {
  return {
    type: 'group' as const,
    operator: group.operator,
    negated: group.negated,
    nodes: group.nodes,
  }
}

export function sourceConditionNode(accountId: string) {
  return {
    type: 'condition' as const,
    field: 'sourceId' as const,
    operator: 'equals' as const,
    negated: false,
    value: accountId,
  }
}

export function mailboxConditionNode(mailboxId: string) {
  return {
    type: 'condition' as const,
    field: 'mailboxId' as const,
    operator: 'equals' as const,
    negated: false,
    value: mailboxId,
  }
}

export function isSourceConditionForAccount(
  node: MailQueryGroup['nodes'][number] | undefined,
  accountId: string,
): boolean {
  return (
    node?.type === 'condition' &&
    node.field === 'sourceId' &&
    node.operator === 'equals' &&
    !node.negated &&
    typeof node.value === 'string' &&
    node.value === accountId
  )
}

export function isMailboxConditionForMailbox(
  node: MailQueryGroup['nodes'][number] | undefined,
  mailboxId: string,
): boolean {
  return (
    node?.type === 'condition' &&
    node.field === 'mailboxId' &&
    node.operator === 'equals' &&
    !node.negated &&
    typeof node.value === 'string' &&
    node.value === mailboxId
  )
}

export function extractAccountIdFromRule(
  rule: AutomationRule,
  fallbackAccountId: string,
): string {
  const sourceNode = rule.condition.root.nodes.find(
    (node) =>
      node.type === 'condition' &&
      node.field === 'sourceId' &&
      node.operator === 'equals' &&
      !node.negated &&
      typeof node.value === 'string' &&
      node.value.trim().length > 0,
  )
  return sourceNode?.type === 'condition' &&
    typeof sourceNode.value === 'string'
    ? sourceNode.value
    : fallbackAccountId
}

export function accountScopedCondition(
  rule: MailQueryRule,
  accountId: string,
): MailQueryRule {
  return {
    root: {
      operator: 'all',
      negated: false,
      nodes: [sourceConditionNode(accountId), groupNode(cloneRule(rule).root)],
    },
  }
}
