import type { SmartMailbox, SmartMailboxSummary } from '../../api/types'
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
  editingSmartMailbox: SmartMailbox | SmartMailboxSummary | null
}): string {
  const { target, editingSmartMailbox } = input
  return target === null
    ? 'mailbox:none'
    : target === 'new'
      ? 'mailbox:new'
      : `mailbox:${target}:${'rule' in (editingSmartMailbox ?? {}) ? 'full' : 'summary'}:${editingSmartMailbox?.updatedAt ?? 'pending'}`
}
