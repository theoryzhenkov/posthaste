/**
 * Pure helpers for automation-rule editing: option tables, draft/action
 * construction, completeness checks, display formatting, and value parsing.
 *
 * Extracted from AutomationActionsEditor so the component file holds UI and
 * these (testable, React-free) helpers stand on their own.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 */
import type {
  AccountOverview,
  AutomationAction,
  AutomationRule,
  AutomationTrigger,
} from '../../api/types'
import type { AutomationRuleDraft } from '../../automationRules'
import { defaultEmptyRule } from './helpers'

export const TRIGGER_OPTIONS: Array<{
  value: AutomationTrigger
  label: string
}> = [
  { value: 'messageArrived', label: 'Mail arrives' },
  { value: 'messageChanged', label: 'Mail changes' },
  { value: 'manual', label: 'Manual' },
]

export const ACTION_KIND_OPTIONS: Array<{
  value: AutomationAction['kind']
  label: string
}> = [
  { value: 'applyTag', label: 'Apply tag' },
  { value: 'removeTag', label: 'Remove tag' },
  { value: 'markRead', label: 'Mark read' },
  { value: 'markUnread', label: 'Mark unread' },
  { value: 'flag', label: 'Flag' },
  { value: 'unflag', label: 'Unflag' },
  { value: 'moveToMailbox', label: 'Move to mailbox' },
]

export type AutomationRuleState = 'active' | 'draft'

export interface AutomationRuleItem {
  state: AutomationRuleState
  draft: AutomationRuleDraft
}

export function createRuleId(prefix = 'automation'): string {
  if (globalThis.crypto && 'randomUUID' in globalThis.crypto) {
    return `${prefix}:${globalThis.crypto.randomUUID()}`
  }
  return `${prefix}:${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`
}

export function defaultAction(): AutomationAction {
  return { kind: 'applyTag', tag: '' }
}

export function actionForKind(
  kind: AutomationAction['kind'],
): AutomationAction {
  switch (kind) {
    case 'applyTag':
      return { kind: 'applyTag', tag: '' }
    case 'removeTag':
      return { kind: 'removeTag', tag: '' }
    case 'moveToMailbox':
      return { kind: 'moveToMailbox', mailboxId: '' }
    case 'markRead':
      return { kind: 'markRead' }
    case 'markUnread':
      return { kind: 'markUnread' }
    case 'flag':
      return { kind: 'flag' }
    case 'unflag':
      return { kind: 'unflag' }
  }
}

export function isActionComplete(action: AutomationAction): boolean {
  switch (action.kind) {
    case 'applyTag':
    case 'removeTag':
      return action.tag.trim().length > 0 && !action.tag.trim().startsWith('$')
    case 'moveToMailbox':
      return action.mailboxId.trim().length > 0
    case 'markRead':
    case 'markUnread':
    case 'flag':
    case 'unflag':
      return true
  }
}

export function isDraftComplete(draft: AutomationRuleDraft): boolean {
  return (
    draft.accountId.trim().length > 0 &&
    draft.name.trim().length > 0 &&
    draft.actions.length > 0 &&
    draft.actions.every(isActionComplete)
  )
}

export function defaultDraft({
  accountId,
  name,
  idPrefix,
}: {
  accountId: string
  name: string
  idPrefix?: string
}): AutomationRuleDraft {
  return {
    id: createRuleId(idPrefix),
    accountId,
    name,
    enabled: true,
    triggers: ['messageArrived'],
    condition: defaultEmptyRule(),
    actions: [defaultAction()],
    backfill: true,
  }
}

export function actionListDescription(items: AutomationRuleItem[]): string {
  if (items.length === 0) {
    return 'No actions configured.'
  }
  const actionCount = items.reduce(
    (count, item) => count + item.draft.actions.length,
    0,
  )
  const draftCount = items.filter((item) => item.state === 'draft').length
  const base = `${actionCount} ${actionCount === 1 ? 'action' : 'actions'} in ${items.length} ${
    items.length === 1 ? 'rule' : 'rules'
  }.`
  return draftCount > 0
    ? `${base} ${draftCount} ${draftCount === 1 ? 'draft' : 'drafts'}.`
    : base
}

export function triggerLabel(trigger: AutomationTrigger): string {
  return (
    TRIGGER_OPTIONS.find((option) => option.value === trigger)?.label ?? trigger
  )
}

export function actionSummary(action: AutomationAction): string {
  switch (action.kind) {
    case 'applyTag':
      return action.tag.trim() ? `Tag ${action.tag.trim()}` : 'Apply tag'
    case 'removeTag':
      return action.tag.trim() ? `Remove ${action.tag.trim()}` : 'Remove tag'
    case 'markRead':
      return 'Mark read'
    case 'markUnread':
      return 'Mark unread'
    case 'flag':
      return 'Flag'
    case 'unflag':
      return 'Unflag'
    case 'moveToMailbox':
      return action.mailboxId.trim()
        ? `Move to ${action.mailboxId.trim()}`
        : 'Move to mailbox'
  }
}

export function ruleActionSummary(draft: AutomationRuleDraft): string {
  if (draft.actions.length === 0) {
    return 'No actions'
  }
  const [firstAction] = draft.actions
  const first = actionSummary(firstAction)
  if (draft.actions.length === 1) {
    return first
  }
  return `${first} +${draft.actions.length - 1}`
}

export function accountName(
  accounts: AccountOverview[],
  accountId: string,
): string {
  return (
    accounts.find((account) => account.id === accountId)?.name ||
    accountId.trim() ||
    'Account'
  )
}

export function upsertRule(rules: AutomationRule[], rule: AutomationRule) {
  return [...rules.filter((candidate) => candidate.id !== rule.id), rule]
}

export function removeRule(rules: AutomationRule[], ruleId: string) {
  return rules.filter((rule) => rule.id !== ruleId)
}

export function parseTrigger(
  value: string,
  fallback: AutomationTrigger,
): AutomationTrigger {
  return (
    TRIGGER_OPTIONS.find((option) => option.value === value)?.value ?? fallback
  )
}

export function parseActionKind(
  value: string,
  fallback: AutomationAction['kind'],
): AutomationAction['kind'] {
  return (
    ACTION_KIND_OPTIONS.find((option) => option.value === value)?.value ??
    fallback
  )
}
