import type {
  AutomationAction,
  AutomationTrigger,
  SmartMailboxRule,
} from '../api/types'

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
