import type { ComposeIntent } from '../composeIntent'
import {
  attachmentSurface,
  categoryForSettingsTarget,
  composeSurface,
  settingsSurface,
} from './factories'
import {
  SettingsSurfaceTargetKind,
  SETTINGS_SURFACE_CATEGORIES,
  type AttachmentSurfaceDescriptor,
  type MessageSurfaceDescriptor,
  type SettingsSurfaceCategory,
  type SettingsSurfaceDescriptor,
  type SettingsSurfaceTarget,
  type SurfaceDescriptor,
} from './types'

type SettingsTargetParamKey =
  | 'accountId'
  | 'smartMailboxId'
  | 'sourceAccountId'
  | 'sourceMailboxId'

export function parseSurfaceRoute(route: string): SurfaceDescriptor | null {
  let url: URL
  try {
    url = new URL(route, 'http://posthaste.local')
  } catch {
    return null
  }
  const parts = url.pathname.split('/').filter(Boolean)
  if (parts.length !== 2 || parts[0] !== 'surface') {
    return null
  }

  switch (parts[1]) {
    case 'message':
      return parseMessageSurface(url.searchParams)
    case 'attachment':
      return parseAttachmentSurface(url.searchParams)
    case 'compose': {
      const intent = parseComposeIntent(url.searchParams)
      return intent ? composeSurface(intent) : null
    }
    case 'settings':
      return parseSettingsSurface(url.searchParams)
    default:
      return null
  }
}

function parseMessageSurface(
  params: URLSearchParams,
): MessageSurfaceDescriptor | null {
  if (
    !hasOnlySurfaceParams(params, ['conversationId', 'sourceId', 'messageId'])
  ) {
    return null
  }
  const conversationId = params.get('conversationId')
  const sourceId = params.get('sourceId')
  const messageId = params.get('messageId')
  if (!conversationId || !sourceId || !messageId) {
    return null
  }
  return {
    kind: 'message',
    disposition: 'focused',
    params: { conversationId, sourceId, messageId },
  }
}

function parseAttachmentSurface(
  params: URLSearchParams,
): AttachmentSurfaceDescriptor | null {
  if (
    !hasOnlySurfaceParams(params, ['sourceId', 'messageId', 'attachmentId'])
  ) {
    return null
  }
  const sourceId = params.get('sourceId')
  const messageId = params.get('messageId')
  const attachmentId = params.get('attachmentId')
  if (!sourceId || !messageId || !attachmentId) {
    return null
  }
  return attachmentSurface({ sourceId, messageId, attachmentId })
}

function parseSettingsSurface(
  params: URLSearchParams,
): SettingsSurfaceDescriptor | null {
  const category = params.get('category')
  if (category !== null && !isSettingsSurfaceCategory(category)) {
    return null
  }
  if (
    !params.has('targetKind') &&
    !hasOnlySurfaceParams(params, ['category'])
  ) {
    return null
  }
  const target = parseSettingsTarget(params)
  if (target === undefined) {
    return null
  }
  if (target && category && category !== categoryForSettingsTarget(target)) {
    return null
  }
  return settingsSurface({ category: category ?? undefined, target })
}

function parseComposeIntent(params: URLSearchParams): ComposeIntent | null {
  const composeKind = params.get('composeKind')
  const sourceId = params.get('sourceId')
  if (!composeKind || !sourceId) {
    return null
  }

  switch (composeKind) {
    case 'new':
      return hasOnlyComposeParams(params, []) ? { kind: 'new', sourceId } : null
    case 'reply': {
      const messageId = params.get('messageId')
      return messageId && hasOnlyComposeParams(params, ['messageId'])
        ? { kind: 'reply', sourceId, messageId }
        : null
    }
    case 'replyAll': {
      const messageId = params.get('messageId')
      return messageId && hasOnlyComposeParams(params, ['messageId'])
        ? { kind: 'replyAll', sourceId, messageId }
        : null
    }
    case 'forward': {
      const messageId = params.get('messageId')
      return messageId && hasOnlyComposeParams(params, ['messageId'])
        ? { kind: 'forward', sourceId, messageId }
        : null
    }
    default:
      return null
  }
}

function hasOnlyComposeParams(
  params: URLSearchParams,
  expectedKeys: readonly string[],
): boolean {
  return hasOnlySurfaceParams(params, [
    'composeKind',
    'sourceId',
    ...expectedKeys,
  ])
}

function parseSettingsTarget(
  params: URLSearchParams,
): SettingsSurfaceTarget | null | undefined {
  const targetKind = params.get('targetKind')
  if (targetKind === null) {
    return null
  }

  switch (targetKind) {
    case SettingsSurfaceTargetKind.Account: {
      if (!hasOnlySettingsTargetParams(params, ['accountId'])) {
        return undefined
      }
      const accountId = params.get('accountId')
      return accountId
        ? { kind: SettingsSurfaceTargetKind.Account, accountId }
        : undefined
    }
    case SettingsSurfaceTargetKind.NewAccount:
      if (!hasOnlySettingsTargetParams(params, [])) {
        return undefined
      }
      return { kind: SettingsSurfaceTargetKind.NewAccount }
    case SettingsSurfaceTargetKind.SmartMailbox: {
      if (!hasOnlySettingsTargetParams(params, ['smartMailboxId'])) {
        return undefined
      }
      const smartMailboxId = params.get('smartMailboxId')
      return smartMailboxId
        ? { kind: SettingsSurfaceTargetKind.SmartMailbox, smartMailboxId }
        : undefined
    }
    case SettingsSurfaceTargetKind.NewSmartMailbox:
      if (!hasOnlySettingsTargetParams(params, [])) {
        return undefined
      }
      return { kind: SettingsSurfaceTargetKind.NewSmartMailbox }
    case SettingsSurfaceTargetKind.SourceMailbox: {
      if (
        !hasOnlySettingsTargetParams(params, [
          'sourceAccountId',
          'sourceMailboxId',
        ])
      ) {
        return undefined
      }
      const sourceAccountId = params.get('sourceAccountId')
      const sourceMailboxId = params.get('sourceMailboxId')
      return sourceAccountId && sourceMailboxId
        ? {
            kind: SettingsSurfaceTargetKind.SourceMailbox,
            sourceAccountId,
            sourceMailboxId,
          }
        : undefined
    }
    default:
      return undefined
  }
}

function hasOnlySettingsTargetParams(
  params: URLSearchParams,
  allowed: readonly SettingsTargetParamKey[],
): boolean {
  return hasOnlySurfaceParams(params, ['category', 'targetKind', ...allowed])
}

function hasOnlySurfaceParams(
  params: URLSearchParams,
  allowed: readonly string[],
): boolean {
  const keys = [...params.keys()]
  return (
    keys.every((key) => allowed.includes(key)) &&
    new Set(keys).size === keys.length
  )
}

function isSettingsSurfaceCategory(
  value: string,
): value is SettingsSurfaceCategory {
  return (SETTINGS_SURFACE_CATEGORIES as readonly string[]).includes(value)
}
