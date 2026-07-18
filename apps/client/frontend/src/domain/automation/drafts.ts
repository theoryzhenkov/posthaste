import type {
  AutomationAction,
  AutomationRule,
  MailQueryRule,
} from '../../data/transport/api/index'
import {
  accountScopedCondition,
  cloneRule,
  isSourceConditionForAccount,
} from './conditions'
import type { AutomationRuleDraft } from './types'

export function actionConditionFromAccountRule(
  rule: AutomationRule,
  accountId: string,
): MailQueryRule {
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
