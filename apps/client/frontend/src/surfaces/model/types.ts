import type { ComposeIntent } from '../../domain/composeIntent'

export type SurfaceDisposition = 'focused'

/** Every settings surface category. This array is the single source of truth —
 *  the {@link SettingsSurfaceCategory} type and the route validator both derive
 *  from it, so the rail, the type, and the parser can never list different
 *  categories (the bug that dropped `tags` from the validator). */
export const SETTINGS_SURFACE_CATEGORIES = [
  'general',
  'appearance',
  'accounts',
  'outbox',
  'mailboxes',
  'automations',
  'tags',
  'storage',
  'notifications',
  'troubleshooting',
] as const

export type SettingsSurfaceCategory =
  (typeof SETTINGS_SURFACE_CATEGORIES)[number]

export const SettingsSurfaceTargetKind = {
  Account: 'account',
  NewAccount: 'newAccount',
  SmartMailbox: 'smartMailbox',
  NewSmartMailbox: 'newSmartMailbox',
  SourceMailbox: 'sourceMailbox',
} as const

export type SettingsSurfaceTargetKind =
  (typeof SettingsSurfaceTargetKind)[keyof typeof SettingsSurfaceTargetKind]

export type SettingsSurfaceTarget =
  | { kind: typeof SettingsSurfaceTargetKind.Account; accountId: string }
  | { kind: typeof SettingsSurfaceTargetKind.NewAccount }
  | {
      kind: typeof SettingsSurfaceTargetKind.SmartMailbox
      smartMailboxId: string
    }
  | { kind: typeof SettingsSurfaceTargetKind.NewSmartMailbox }
  | {
      kind: typeof SettingsSurfaceTargetKind.SourceMailbox
      sourceAccountId: string
      sourceMailboxId: string
    }

export interface MessageSurfaceDescriptor {
  kind: 'message'
  disposition: SurfaceDisposition
  params: {
    conversationId: string
    sourceId: string
    messageId: string
  }
}

export interface AttachmentSurfaceDescriptor {
  kind: 'attachment'
  disposition: SurfaceDisposition
  params: {
    sourceId: string
    messageId: string
    attachmentId: string
  }
}

export interface SettingsSurfaceDescriptor {
  kind: 'settings'
  disposition: SurfaceDisposition
  params: {
    category?: SettingsSurfaceCategory
    target?: SettingsSurfaceTarget | null
  }
}

export interface ComposeSurfaceDescriptor {
  kind: 'compose'
  disposition: SurfaceDisposition
  params: ComposeIntent
}

export type SurfaceDescriptor =
  | MessageSurfaceDescriptor
  | AttachmentSurfaceDescriptor
  | SettingsSurfaceDescriptor
  | ComposeSurfaceDescriptor

export type SurfaceRouteState =
  | { kind: 'none' }
  | { kind: 'valid'; route: string; surface: SurfaceDescriptor }
  | { kind: 'invalid'; route: string }

export interface SurfaceLocation {
  hash: string
  pathname: string
  search: string
}
