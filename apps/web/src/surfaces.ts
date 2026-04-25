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

export function parseSurfaceRoute(route: string): SurfaceDescriptor | null {
  const url = new URL(route, 'http://posthaste.local')
  const parts = url.pathname.split('/').filter(Boolean)
  if (parts.length !== 2 || parts[0] !== 'surface') {
    return null
  }

  switch (parts[1]) {
    case 'message': {
      const conversationId = url.searchParams.get('conversationId')
      const sourceId = url.searchParams.get('sourceId')
      const messageId = url.searchParams.get('messageId')
      if (!conversationId || !sourceId || !messageId) {
        return null
      }
      return {
        kind: 'message',
        disposition: 'focused',
        params: { conversationId, sourceId, messageId },
      }
    }
    case 'settings': {
      const category = url.searchParams.get('category')
      if (category !== null && !isSettingsSurfaceCategory(category)) {
        return null
      }
      return settingsSurface({
        category: category ?? undefined,
        accountId: url.searchParams.get('accountId'),
        smartMailboxId: url.searchParams.get('smartMailboxId'),
      })
    }
    default:
      return null
  }
}

export function surfaceFromLocation(
  location: Location,
): SurfaceDescriptor | null {
  const hashRoute = location.hash.startsWith('#') ? location.hash.slice(1) : ''
  const route =
    hashRoute.length > 0 ? hashRoute : `${location.pathname}${location.search}`
  return parseSurfaceRoute(route)
}

function isSettingsSurfaceCategory(
  value: string,
): value is SettingsSurfaceCategory {
  return (
    value === 'general' ||
    value === 'appearance' ||
    value === 'accounts' ||
    value === 'mailboxes'
  )
}
