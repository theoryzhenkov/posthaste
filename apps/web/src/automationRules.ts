export {
  accountScopedCondition,
  cloneRule,
  extractAccountIdFromRule,
  groupNode,
  isMailboxConditionForMailbox,
  isSourceConditionForAccount,
  mailboxConditionNode,
  sourceConditionNode,
} from './automation-rules/conditions'
export {
  actionConditionFromAccountRule,
  draftToRule,
  normalizeAction,
  ruleToDraft,
} from './automation-rules/drafts'
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
} from './automation-rules/linked'
export type { AutomationRuleDraft } from './automation-rules/types'
