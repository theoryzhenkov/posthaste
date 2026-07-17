import type { ComposeIntent } from '../composeIntent'
import type { MailSelection } from '@/data/selection'
import {
  SettingsSurfaceTargetKind,
  type AttachmentSurfaceDescriptor,
  type ComposeSurfaceDescriptor,
  type MessageSurfaceDescriptor,
  type SettingsSurfaceCategory,
  type SettingsSurfaceDescriptor,
  type SettingsSurfaceTarget,
} from './types'

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

export function categoryForSettingsTarget(
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
