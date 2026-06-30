import type { ComposeIntent } from '../composeIntent'

export type SurfaceDisposition = 'focused'
export type SettingsSurfaceCategory =
  | 'general'
  | 'appearance'
  | 'accounts'
  | 'outbox'
  | 'mailboxes'
  | 'storage'
  | 'notifications'
  | 'troubleshooting'

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
