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
  mailboxColors: MailboxColor[]
}

/**
 * A per-mailbox sidebar color override (presentation only). Overrides the
 * renderer's default hash-derived color for `(sourceId, mailboxId)`.
 */
export interface MailboxColor {
  sourceId: string
  mailboxId: string
  /** Color hue (0–360). */
  hue: number
}

/** @spec docs/L1-sync#local-cache-planning */
export interface CachePolicy {
  softCapBytes: number
  hardCapBytes: number
  cacheBodies: boolean
  cacheRawMessages: boolean
  cacheAttachments: boolean
}
