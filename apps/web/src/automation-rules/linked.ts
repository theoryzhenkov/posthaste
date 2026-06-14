import type { AutomationRule, SmartMailbox, SmartMailboxRule } from '../api/types'
import {
  accountScopedCondition,
  cloneRule,
  extractAccountIdFromRule,
  groupNode,
  isMailboxConditionForMailbox,
  isSourceConditionForAccount,
  mailboxConditionNode,
  sourceConditionNode,
} from './conditions'
import { draftToRule, ruleToDraft } from './drafts'
import type { AutomationRuleDraft } from './types'

export const SMART_MAILBOX_RULE_PREFIX = 'smart-mailbox'
export const SOURCE_MAILBOX_RULE_PREFIX = 'source-mailbox'

export function accountRulePrefix(accountId: string): string {
  return `account:${accountId}:`
}

export function sourceMailboxRulePrefix(
  accountId: string,
  mailboxId: string,
): string {
  return `${SOURCE_MAILBOX_RULE_PREFIX}:${accountId}:${mailboxId}:`
}

export function smartMailboxRulePrefix(smartMailboxId: string): string {
  return `${SMART_MAILBOX_RULE_PREFIX}:${smartMailboxId}:`
}

export function isSourceMailboxLinkedRule(
  rule: AutomationRule,
  accountId: string,
  mailboxId: string,
): boolean {
  return rule.id.startsWith(sourceMailboxRulePrefix(accountId, mailboxId))
}

export function isSmartMailboxLinkedRule(
  rule: AutomationRule,
  smartMailboxId: string,
): boolean {
  return rule.id.startsWith(smartMailboxRulePrefix(smartMailboxId))
}

export function actionConditionFromSourceMailboxRule(
  rule: AutomationRule,
  accountId: string,
  mailboxId: string,
): SmartMailboxRule {
  const nodes = rule.condition.root.nodes
  const actionNode = nodes[2]
  if (
    rule.condition.root.operator === 'all' &&
    !rule.condition.root.negated &&
    isSourceConditionForAccount(nodes[0], accountId) &&
    isMailboxConditionForMailbox(nodes[1], mailboxId) &&
    actionNode?.type === 'group'
  ) {
    return {
      root: {
        operator: actionNode.operator,
        negated: actionNode.negated,
        nodes: actionNode.nodes,
      },
    }
  }
  return cloneRule(rule.condition)
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

export function sourceMailboxDraftToRule(
  draft: AutomationRuleDraft,
  mailboxId: string,
): AutomationRule {
  return {
    ...draftToRule(draft),
    condition: {
      root: {
        operator: 'all',
        negated: false,
        nodes: [
          sourceConditionNode(draft.accountId),
          mailboxConditionNode(mailboxId),
          groupNode(cloneRule(draft.condition).root),
        ],
      },
    },
  }
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

export function rewriteSmartMailboxLinkedRules(
  rules: AutomationRule[],
  smartMailbox: SmartMailbox,
): AutomationRule[] {
  let changed = false
  const nextRules = rules.map((rule) => {
    if (!isSmartMailboxLinkedRule(rule, smartMailbox.id)) {
      return rule
    }
    changed = true
    const accountId = extractAccountIdFromRule(rule, '')
    return smartMailboxDraftToRule(
      {
        ...ruleToDraft(accountId, rule),
        condition: actionConditionFromSmartMailboxRule(rule, accountId),
      },
      smartMailbox,
    )
  })
  return changed ? nextRules : rules
}

export function removeSmartMailboxLinkedRules(
  rules: AutomationRule[],
  smartMailboxId: string,
): AutomationRule[] {
  const nextRules = rules.filter(
    (rule) => !isSmartMailboxLinkedRule(rule, smartMailboxId),
  )
  return nextRules.length === rules.length ? rules : nextRules
}
