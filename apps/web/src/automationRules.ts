import type {
  AutomationAction,
  AutomationRule,
  AutomationTrigger,
  SmartMailbox,
  SmartMailboxGroup,
  SmartMailboxRule,
} from './api/types'

export const SMART_MAILBOX_RULE_PREFIX = 'smart-mailbox'

export interface AutomationRuleDraft {
  id: string
  accountId: string
  name: string
  enabled: boolean
  triggers: AutomationTrigger[]
  condition: SmartMailboxRule
  actions: AutomationAction[]
  backfill: boolean
}

export function cloneRule(rule: SmartMailboxRule): SmartMailboxRule {
  return JSON.parse(JSON.stringify(rule)) as SmartMailboxRule
}

export function groupNode(group: SmartMailboxGroup) {
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

export function isSourceConditionForAccount(
  node: SmartMailboxGroup['nodes'][number] | undefined,
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
  rule: SmartMailboxRule,
  accountId: string,
): SmartMailboxRule {
  return {
    root: {
      operator: 'all',
      negated: false,
      nodes: [sourceConditionNode(accountId), groupNode(cloneRule(rule).root)],
    },
  }
}

export function actionConditionFromAccountRule(
  rule: AutomationRule,
  accountId: string,
): SmartMailboxRule {
  const nodes = rule.condition.root.nodes
  const secondNode = nodes[1]
  if (
    rule.condition.root.operator === 'all' &&
    !rule.condition.root.negated &&
    isSourceConditionForAccount(nodes[0], accountId) &&
    secondNode?.type === 'group'
  ) {
    return {
      root: {
        operator: secondNode.operator,
        negated: secondNode.negated,
        nodes: secondNode.nodes,
      },
    }
  }
  return cloneRule(rule.condition)
}

export function normalizeAction(action: AutomationAction): AutomationAction {
  switch (action.kind) {
    case 'applyTag':
      return { kind: 'applyTag', tag: action.tag.trim() }
    case 'removeTag':
      return { kind: 'removeTag', tag: action.tag.trim() }
    case 'moveToMailbox':
      return { kind: 'moveToMailbox', mailboxId: action.mailboxId.trim() }
    case 'markRead':
    case 'markUnread':
    case 'flag':
    case 'unflag':
      return action
  }
}

export function ruleToDraft(
  accountId: string,
  rule: AutomationRule,
): AutomationRuleDraft {
  return {
    id: rule.id,
    accountId,
    name: rule.name,
    enabled: rule.enabled,
    triggers: rule.triggers.length > 0 ? rule.triggers : ['messageArrived'],
    condition: actionConditionFromAccountRule(rule, accountId),
    actions: rule.actions.map(normalizeAction),
    backfill: rule.backfill,
  }
}

export function draftToRule(draft: AutomationRuleDraft): AutomationRule {
  return {
    id: draft.id.trim(),
    name: draft.name.trim(),
    enabled: draft.enabled,
    triggers: draft.triggers.length > 0 ? draft.triggers : ['messageArrived'],
    condition: accountScopedCondition(draft.condition, draft.accountId),
    actions: draft.actions.map(normalizeAction),
    backfill: draft.backfill,
  }
}

export function accountRulePrefix(accountId: string): string {
  return `account:${accountId}:`
}

export function smartMailboxRulePrefix(smartMailboxId: string): string {
  return `${SMART_MAILBOX_RULE_PREFIX}:${smartMailboxId}:`
}

export function isSmartMailboxLinkedRule(
  rule: AutomationRule,
  smartMailboxId: string,
): boolean {
  return rule.id.startsWith(smartMailboxRulePrefix(smartMailboxId))
}

export function actionConditionFromSmartMailboxRule(
  rule: AutomationRule,
  accountId: string,
): SmartMailboxRule {
  const nodes = rule.condition.root.nodes
  const smartMailboxAndActionNode = nodes[1]
  if (
    rule.condition.root.operator === 'all' &&
    !rule.condition.root.negated &&
    isSourceConditionForAccount(nodes[0], accountId) &&
    smartMailboxAndActionNode?.type === 'group' &&
    smartMailboxAndActionNode.operator === 'all' &&
    !smartMailboxAndActionNode.negated &&
    smartMailboxAndActionNode.nodes[1]?.type === 'group'
  ) {
    const actionNode = smartMailboxAndActionNode.nodes[1]
    return {
      root: {
        operator: actionNode.operator,
        negated: actionNode.negated,
        nodes: actionNode.nodes,
      },
    }
  }

  const thirdNode = nodes[2]
  if (
    rule.condition.root.operator === 'all' &&
    !rule.condition.root.negated &&
    isSourceConditionForAccount(nodes[0], accountId) &&
    nodes[1]?.type === 'group' &&
    thirdNode?.type === 'group'
  ) {
    return {
      root: {
        operator: thirdNode.operator,
        negated: thirdNode.negated,
        nodes: thirdNode.nodes,
      },
    }
  }
  return cloneRule(rule.condition)
}

export function smartMailboxDraftToRule(
  draft: AutomationRuleDraft,
  smartMailbox: SmartMailbox,
): AutomationRule {
  return {
    ...draftToRule(draft),
    condition: accountScopedCondition(
      {
        root: {
          operator: 'all',
          negated: false,
          nodes: [
            groupNode(cloneRule(smartMailbox.rule).root),
            groupNode(cloneRule(draft.condition).root),
          ],
        },
      },
      draft.accountId,
    ),
  }
}
