import {
  SettingsSurfaceTargetKind,
  type SettingsSurfaceTarget,
} from '@/domain/surface'
import type { MailboxEditorTarget } from './types'
import type { EditorTarget } from './types'

export function accountEditorTargetFromSettingsTarget(
  target: SettingsSurfaceTarget | null,
): EditorTarget | null {
  if (!target) {
    return null
  }

  switch (target.kind) {
    case SettingsSurfaceTargetKind.Account:
      return target.accountId
    case SettingsSurfaceTargetKind.NewAccount:
      return 'new'
    case SettingsSurfaceTargetKind.SmartMailbox:
    case SettingsSurfaceTargetKind.NewSmartMailbox:
    case SettingsSurfaceTargetKind.SourceMailbox:
      return null
  }
}

export function mailboxEditorTargetFromSettingsTarget(
  target: SettingsSurfaceTarget | null,
): MailboxEditorTarget | null {
  if (!target) {
    return null
  }

  switch (target.kind) {
    case SettingsSurfaceTargetKind.SmartMailbox:
      return { kind: 'smart', id: target.smartMailboxId }
    case SettingsSurfaceTargetKind.NewSmartMailbox:
      return { kind: 'smart', id: 'new' }
    case SettingsSurfaceTargetKind.SourceMailbox:
      return {
        kind: 'source',
        accountId: target.sourceAccountId,
        mailboxId: target.sourceMailboxId,
      }
    case SettingsSurfaceTargetKind.Account:
    case SettingsSurfaceTargetKind.NewAccount:
      return null
  }
}
