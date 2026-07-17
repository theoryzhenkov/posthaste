import {
  SettingsSurfaceTargetKind,
  type AttachmentSurfaceDescriptor,
  type ComposeSurfaceDescriptor,
  type MessageSurfaceDescriptor,
  type SettingsSurfaceDescriptor,
  type SurfaceDescriptor,
} from './types'

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
  if (surface.params.kind === 'mailto') {
    params.set('mailtoUri', surface.params.mailtoUri)
  } else if (surface.params.kind !== 'new') {
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
