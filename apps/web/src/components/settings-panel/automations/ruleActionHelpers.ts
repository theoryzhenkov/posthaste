/**
 * Pure helpers for the automation action editor (kept out of the component file
 * so fast-refresh's "only export components" rule holds).
 */
import type { WritableRuleAction } from '../../../api/types'

export type ActionKind = WritableRuleAction['kind']

/** The safe default when switching to a given action kind. */
export function defaultActionForKind(kind: ActionKind): WritableRuleAction {
  switch (kind) {
    case 'tag':
      return { kind: 'tag', tag: '' }
    case 'move':
      return { kind: 'move', mailboxId: '' }
    case 'notify':
      return { kind: 'notify', title: '', body: '' }
    case 'emit':
      return { kind: 'emit' }
    case 'webhook':
      // Least-grant default (threat 2 / ruling 23): read only.
      return { kind: 'webhook', url: '', grants: ['read'], expirySeconds: 3600 }
  }
}

/** A one-line human summary of an action for the rule list. */
export function actionSummary(
  action: WritableRuleAction | { kind: string },
): string {
  switch (action.kind) {
    case 'tag':
      return `Tag "${(action as { tag: string }).tag || '…'}"`
    case 'move':
      return `Move to ${(action as { mailboxId: string }).mailboxId || '…'}`
    case 'notify':
      return `Notify: ${(action as { title: string }).title || '…'}`
    case 'emit':
      return 'Emit rule.fired'
    case 'webhook':
      return `Webhook → ${(action as { url: string }).url || '…'}`
    case 'exec':
      return 'Exec (config file)'
    default:
      return action.kind
  }
}
