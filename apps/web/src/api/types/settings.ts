import type { Appearance } from './appearance'
import type { AutomationRule } from './automation'
import type { Notifications } from './notifications'

export interface AppSettings {
  defaultAccountId: string | null
  cachePolicy: CachePolicy
  automationRules: AutomationRule[]
  automationDrafts: AutomationRule[]
  appearance?: Appearance | null
  notifications?: Notifications | null
}

/** @spec docs/L1-sync#local-cache-planning */
export interface CachePolicy {
  softCapBytes: number
  hardCapBytes: number
  cacheBodies: boolean
  cacheRawMessages: boolean
  cacheAttachments: boolean
}
