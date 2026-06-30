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
  /** Per-tag presentation overrides (color + icon), keyed by name. Pure
   *  presentation; absent fields fall back to name-derived defaults. */
  tags: TagAppearance[]
  /** Explicit sidebar arrangement of smart mailboxes (ids); an override list,
   *  ids absent fall back to the canonical/default order. */
  smartMailboxOrder: string[]
  /** Explicit sidebar arrangement of accounts (ids); same override semantics. */
  accountOrder: string[]
}

/**
 * Sparse settings patch payload. `forceBackfill` is a transient command flag
 * (not persisted state): when true, the backend re-runs the current backfill
 * rules against existing messages after saving.
 */
export interface PatchSettingsInput extends Partial<AppSettings> {
  forceBackfill?: boolean
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

/**
 * A per-tag presentation override (presentation only), keyed by tag `name`.
 * Each optional field overrides the renderer's name-derived default for that
 * aspect; absent fields keep the default.
 */
export interface TagAppearance {
  /** The tag (keyword) this override applies to. */
  name: string
  /** Foreground/text color (CSS color string). */
  fg?: string | null
  /** Background color (CSS color string). */
  bg?: string | null
  /** Lucide icon name (e.g. `briefcase`). */
  icon?: string | null
}

/** @spec docs/L1-sync#local-cache-planning */
export interface CachePolicy {
  softCapBytes: number
  hardCapBytes: number
  cacheBodies: boolean
  cacheRawMessages: boolean
  cacheAttachments: boolean
}
