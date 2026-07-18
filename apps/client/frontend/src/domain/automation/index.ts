export {
  accountScopedCondition,
  cloneRule,
  extractAccountIdFromRule,
  groupNode,
  isMailboxConditionForMailbox,
  isSourceConditionForAccount,
  mailboxConditionNode,
  sourceConditionNode,
} from './conditions'
export {
  actionConditionFromAccountRule,
  draftToRule,
  normalizeAction,
  ruleToDraft,
} from './drafts'
export {
  SMART_MAILBOX_RULE_PREFIX,
  SOURCE_MAILBOX_RULE_PREFIX,
  accountRulePrefix,
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
