import type { AutomationRule, MailQueryRule } from '../../../../data/transport/api/index'
import { ruleToDraft } from '../../../../domain/automation/index'
import type {
  AutomationRuleItem,
  AutomationRuleState,
} from '../model'

export function linkedAutomationRuleItems({
  rules,
  drafts,
  isLinkedRule,
  accountIdForRule,
  conditionForRule,
}: {
  rules: AutomationRule[]
  drafts: AutomationRule[]
  isLinkedRule: (rule: AutomationRule) => boolean
  accountIdForRule: (rule: AutomationRule) => string
  conditionForRule: (rule: AutomationRule, accountId: string) => MailQueryRule
}): AutomationRuleItem[] {
  const mapItem =
    (state: AutomationRuleState) =>
    (rule: AutomationRule): AutomationRuleItem => {
      const accountId = accountIdForRule(rule)
      return {
        state,
        draft: {
          ...ruleToDraft(accountId, rule),
          condition: conditionForRule(rule, accountId),
        },
      }
    }

  return [
    ...rules.filter(isLinkedRule).map(mapItem('active')),
    ...drafts.filter(isLinkedRule).map(mapItem('draft')),
  ]
}
