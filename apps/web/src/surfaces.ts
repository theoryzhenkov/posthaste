import type { MailSelection } from './mailState'

export type SurfaceDisposition = 'focused'
export type SettingsSurfaceCategory =
  | 'general'
  | 'appearance'
  | 'accounts'
  | 'mailboxes'

export interface MessageSurfaceDescriptor {
  kind: 'message'
  disposition: SurfaceDisposition
  params: {
    conversationId: string
    sourceId: string
    messageId: string
  }
}

export interface SettingsSurfaceDescriptor {
  kind: 'settings'
  disposition: SurfaceDisposition
  params: {
    category?: SettingsSurfaceCategory
    accountId?: string | null
    smartMailboxId?: string | null
  }
}

export type SurfaceDescriptor =
  | MessageSurfaceDescriptor
  | SettingsSurfaceDescriptor

export function messageSurfaceFromSelection(
  selection: MailSelection,
): MessageSurfaceDescriptor {
  return {
    kind: 'message',
    disposition: 'focused',
    params: {
      conversationId: selection.conversationId,
      sourceId: selection.sourceId,
      messageId: selection.messageId,
    },
  }
}

export function settingsSurface(input?: {
  category?: SettingsSurfaceCategory
  accountId?: string | null
  smartMailboxId?: string | null
}): SettingsSurfaceDescriptor {
  return {
    kind: 'settings',
    disposition: 'focused',
    params: {
      category: input?.category,
      accountId: input?.accountId ?? null,
      smartMailboxId: input?.smartMailboxId ?? null,
    },
  }
}

export function surfaceRoute(surface: SurfaceDescriptor): string {
  const params = new URLSearchParams()
  for (const [key, value] of Object.entries(surface.params)) {
    if (value !== undefined && value !== null) {
      params.set(key, value)
    }
  }
  const query = params.toString()
  return `/surface/${surface.kind}${query ? `?${query}` : ''}`
}
