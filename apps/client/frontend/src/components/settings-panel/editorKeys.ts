import type { SmartMailboxRow } from '@/gen'
import type { EditorTarget } from './types'

export function accountEditorKey(target: EditorTarget | null): string {
  return target === null
    ? 'account:none'
    : target === 'new'
      ? 'account:new'
      : `account:${target}`
}

export function smartMailboxEditorKey(input: {
  target: string | null
  editingSmartMailbox: SmartMailboxRow | null
}): string {
  const { target, editingSmartMailbox } = input
  return target === null
    ? 'mailbox:none'
    : target === 'new'
      ? 'mailbox:new'
      : `mailbox:${target}:${editingSmartMailbox?.updatedAt ?? 'pending'}`
}
