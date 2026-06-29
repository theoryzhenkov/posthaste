import type { Appearance } from './appearance'
import type { AutomationRule } from './automation'

export interface AppSettings {
  defaultAccountId: string | null
  cachePolicy: CachePolicy
  automationRules: AutomationRule[]
  automationDrafts: AutomationRule[]
  appearance?: Appearance | null
}

/** @spec docs/L1-sync#local-cache-planning */
export interface CachePolicy {
  softCapBytes: number
  hardCapBytes: number
  cacheBodies: boolean
  cacheRawMessages: boolean
  cacheAttachments: boolean
}
