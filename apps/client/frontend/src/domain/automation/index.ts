export { extractAccountIdFromRule } from './conditions'
export { actionConditionFromAccountRule, draftToRule, ruleToDraft } from './drafts'
export {
  actionConditionFromSmartMailboxRule,
  actionConditionFromSourceMailboxRule,
  isSmartMailboxLinkedRule,
  isSourceMailboxLinkedRule,
  removeSmartMailboxLinkedRules,
  rewriteSmartMailboxLinkedRules,
  smartMailboxDraftToRule,
  smartMailboxRulePrefix,
  sourceMailboxDraftToRule,
  sourceMailboxRulePrefix,
} from './linked'
export type { AutomationRuleDraft } from './types'
