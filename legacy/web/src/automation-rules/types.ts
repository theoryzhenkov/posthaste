import type {
  AutomationAction,
  AutomationTrigger,
  MailQueryRule,
} from '../api/types'

export interface AutomationRuleDraft {
  id: string
  accountId: string
  name: string
  enabled: boolean
  triggers: AutomationTrigger[]
  condition: MailQueryRule
  actions: AutomationAction[]
  backfill: boolean
}
