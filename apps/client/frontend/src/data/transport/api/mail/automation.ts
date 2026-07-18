import type { MessageSummary } from './mail'
import type { MailQueryRule } from './mailQuery'

export type AutomationTrigger = 'messageArrived' | 'messageChanged' | 'manual'

export type AutomationAction =
  | { kind: 'applyTag'; tag: string }
  | { kind: 'removeTag'; tag: string }
  | { kind: 'markRead' }
  | { kind: 'markUnread' }
  | { kind: 'flag' }
  | { kind: 'unflag' }
  | { kind: 'moveToMailbox'; mailboxId: string }

export interface AutomationRule {
  id: string
  name: string
  enabled: boolean
  triggers: AutomationTrigger[]
  condition: MailQueryRule
  actions: AutomationAction[]
  backfill: boolean
}

export interface AutomationRulePreviewInput {
  condition: MailQueryRule
  limit?: number
}

export interface AutomationRulePreviewResponse {
  total: number
  items: MessageSummary[]
}
