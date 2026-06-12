import type { MessageSummary } from './mail'
import type { SmartMailboxRule } from './smartMailboxes'

export type AutomationTrigger = 'messageArrived' | 'messageChanged' | 'manual'

/** @spec docs/L1-api#application-settings */
export type AutomationAction =
  | { kind: 'applyTag'; tag: string }
  | { kind: 'removeTag'; tag: string }
  | { kind: 'markRead' }
  | { kind: 'markUnread' }
  | { kind: 'flag' }
  | { kind: 'unflag' }
  | { kind: 'moveToMailbox'; mailboxId: string }

/** @spec docs/L1-api#application-settings */
export interface AutomationRule {
  id: string
  name: string
  enabled: boolean
  triggers: AutomationTrigger[]
  condition: SmartMailboxRule
  actions: AutomationAction[]
  backfill: boolean
}

/** @spec docs/L1-api#application-settings */
export interface AutomationRulePreviewInput {
  condition: SmartMailboxRule
  limit?: number
}

/** @spec docs/L1-api#application-settings */
export interface AutomationRulePreviewResponse {
  total: number
  items: MessageSummary[]
}
