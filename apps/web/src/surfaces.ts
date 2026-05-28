import type { ComposeIntent } from './composeIntent'
import type { MailSelection } from './mailState'

export type SurfaceDisposition = 'focused'
export type SettingsSurfaceCategory =
  | 'general'
  | 'appearance'
  | 'accounts'
  | 'mailboxes'

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

const SETTINGS_TARGET_PARAM_KEYS = [
  'accountId',
  'smartMailboxId',
  'sourceAccountId',
  'sourceMailboxId',
] as const

type SettingsTargetParamKey = (typeof SETTINGS_TARGET_PARAM_KEYS)[number]

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
  target?: SettingsSurfaceTarget | null
}): SettingsSurfaceDescriptor {
  const target = input?.target ?? null
  const category = target ? categoryForSettingsTarget(target) : input?.category
  const params: SettingsSurfaceDescriptor['params'] = {}
  if (category) {
    params.category = category
  }
  if (target) {
    params.target = target
  }
  return {
    kind: 'settings',
    disposition: 'focused',
    params,
  }
}

export function settingsCategorySurface(
  category: SettingsSurfaceCategory,
): SettingsSurfaceDescriptor {
  return settingsSurface({ category })
}

export function accountSettingsSurface(
  accountId: string,
): SettingsSurfaceDescriptor {
  return settingsSurface({
    target: { kind: SettingsSurfaceTargetKind.Account, accountId },
  })
}

export function newAccountSettingsSurface(): SettingsSurfaceDescriptor {
  return settingsSurface({
    target: { kind: SettingsSurfaceTargetKind.NewAccount },
  })
}

export function smartMailboxSettingsSurface(
  smartMailboxId: string,
): SettingsSurfaceDescriptor {
  return settingsSurface({
    target: { kind: SettingsSurfaceTargetKind.SmartMailbox, smartMailboxId },
  })
}

export function newSmartMailboxSettingsSurface(): SettingsSurfaceDescriptor {
  return settingsSurface({
    target: { kind: SettingsSurfaceTargetKind.NewSmartMailbox },
  })
}

export function sourceMailboxSettingsSurface(
  sourceAccountId: string,
  sourceMailboxId: string,
): SettingsSurfaceDescriptor {
  return settingsSurface({
    target: {
      kind: SettingsSurfaceTargetKind.SourceMailbox,
      sourceAccountId,
      sourceMailboxId,
    },
  })
}

export function composeSurface(
  intent: ComposeIntent,
): ComposeSurfaceDescriptor {
  return {
    kind: 'compose',
    disposition: 'focused',
    params: intent,
  }
}

export function attachmentSurface(input: {
  sourceId: string
  messageId: string
  attachmentId: string
}): AttachmentSurfaceDescriptor {
  return {
    kind: 'attachment',
    disposition: 'focused',
    params: input,
  }
}

export function surfaceRoute(surface: SurfaceDescriptor): string {
  switch (surface.kind) {
    case 'settings':
      return settingsSurfaceRoute(surface)
    case 'compose':
      return composeSurfaceRoute(surface)
    case 'message':
    case 'attachment':
      return genericSurfaceRoute(surface)
  }
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
    case 'attachment': {
      const sourceId = url.searchParams.get('sourceId')
      const messageId = url.searchParams.get('messageId')
      const attachmentId = url.searchParams.get('attachmentId')
      if (!sourceId || !messageId || !attachmentId) {
        return null
      }
      return attachmentSurface({ sourceId, messageId, attachmentId })
    }
    case 'compose': {
      const intent = parseComposeIntent(url.searchParams)
      return intent ? composeSurface(intent) : null
    }
    case 'settings': {
      const category = url.searchParams.get('category')
      if (category !== null && !isSettingsSurfaceCategory(category)) {
        return null
      }
      const target = parseSettingsTarget(url.searchParams)
      if (target === undefined) {
        return null
      }
      if (
        target &&
        category &&
        category !== categoryForSettingsTarget(target)
      ) {
        return null
      }
      return settingsSurface({ category: category ?? undefined, target })
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

function genericSurfaceRoute(
  surface: MessageSurfaceDescriptor | AttachmentSurfaceDescriptor,
): string {
  const params = new URLSearchParams()
  for (const [key, value] of Object.entries(surface.params)) {
    params.set(key, value)
  }
  const query = params.toString()
  return `/surface/${surface.kind}${query ? `?${query}` : ''}`
}

function composeSurfaceRoute(surface: ComposeSurfaceDescriptor): string {
  const params = new URLSearchParams()
  params.set('composeKind', surface.params.kind)
  params.set('sourceId', surface.params.sourceId)
  if (surface.params.kind !== 'new') {
    params.set('messageId', surface.params.messageId)
  }
  return `/surface/compose?${params.toString()}`
}

function settingsSurfaceRoute(surface: SettingsSurfaceDescriptor): string {
  const params = new URLSearchParams()
  if (surface.params.category) {
    params.set('category', surface.params.category)
  }

  const target = surface.params.target
  if (target) {
    params.set('targetKind', target.kind)
    switch (target.kind) {
      case SettingsSurfaceTargetKind.Account:
        params.set('accountId', target.accountId)
        break
      case SettingsSurfaceTargetKind.NewAccount:
        break
      case SettingsSurfaceTargetKind.SmartMailbox:
        params.set('smartMailboxId', target.smartMailboxId)
        break
      case SettingsSurfaceTargetKind.NewSmartMailbox:
        break
      case SettingsSurfaceTargetKind.SourceMailbox:
        params.set('sourceAccountId', target.sourceAccountId)
        params.set('sourceMailboxId', target.sourceMailboxId)
        break
    }
  }

  const query = params.toString()
  return `/surface/settings${query ? `?${query}` : ''}`
}

function categoryForSettingsTarget(
  target: SettingsSurfaceTarget,
): SettingsSurfaceCategory {
  switch (target.kind) {
    case SettingsSurfaceTargetKind.Account:
    case SettingsSurfaceTargetKind.NewAccount:
      return 'accounts'
    case SettingsSurfaceTargetKind.SmartMailbox:
    case SettingsSurfaceTargetKind.NewSmartMailbox:
    case SettingsSurfaceTargetKind.SourceMailbox:
      return 'mailboxes'
  }
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
  return [...params.keys()].every(
    (key) =>
      key === 'composeKind' || key === 'sourceId' || expectedKeys.includes(key),
  )
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
  return SETTINGS_TARGET_PARAM_KEYS.every(
    (key) => allowed.includes(key) || !params.has(key),
  )
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
