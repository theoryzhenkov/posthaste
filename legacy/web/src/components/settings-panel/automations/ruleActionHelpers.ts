/**
 * The rule-action REGISTRY — the web's single source for the writable action
 * vocabulary's presentation (RFC-L2-scripting ruling 23; DESIGN-L2-rule-actions).
 *
 * One entry per writable action kind, exhaustively typed
 * (`Record<ActionKind, …>` over the union {@link WritableRuleAction} derives
 * from the OpenAPI-generated schema): a new wire kind fails the typecheck here
 * until its registry row exists, and a kind removed from the wire strands its
 * row the same way. The picker options, the safe defaults, the one-line
 * summaries, and the destructive labelling all DERIVE from this table — adding
 * action N+1 to the editor is one row here plus its form branch in
 * `RuleActionEditor` (which the compiler also forces, via the same union).
 *
 * SECURITY (the structural exec gate): the registry is keyed by
 * `WritableRuleAction['kind']`, which has NO `exec` member — so the picker
 * cannot offer exec and this module cannot describe it except as the read-only
 * config-file summary below. Kept a plain `.ts` module (no components) so
 * fast-refresh's "only export components" rule holds for the editor.
 */
import type { WritableRuleAction } from '../../../api/types'

export type ActionKind = WritableRuleAction['kind']

export interface ActionDescriptor {
  /** Picker label. */
  label: string
  /** One-line explanation under the picker. */
  hint: string
  /** The safe default value when switching to this kind. */
  defaultAction: () => WritableRuleAction
  /** One-line human summary of a configured action, for the rule list. */
  summary: (action: WritableRuleAction) => string
  /**
   * Irreversibly destroys mail. Drives the unmistakable destructive styling +
   * explanatory copy in the editor and the list badge.
   */
  destructive?: boolean
}

const MOVE_ROLE_LABELS = {
  archive: 'Archive',
  junk: 'Junk',
  trash: 'Trash',
  inbox: 'Inbox',
} as const

export const ACTION_REGISTRY: Record<ActionKind, ActionDescriptor> = {
  tag: {
    label: 'Add a tag',
    hint: 'Tag the matched message.',
    defaultAction: () => ({ kind: 'tag', tag: '' }),
    summary: (action) =>
      action.kind === 'tag' ? `Tag "${action.tag || '…'}"` : '',
  },
  move: {
    label: 'Move to mailbox',
    hint: 'Move it to one specific mailbox (by id).',
    defaultAction: () => ({ kind: 'move', mailboxId: '' }),
    summary: (action) =>
      action.kind === 'move' ? `Move to ${action.mailboxId || '…'}` : '',
  },
  moveToRole: {
    label: 'Move to Archive/Junk/Trash…',
    hint: 'File it into the mailbox with that role, whichever account it lives in.',
    defaultAction: () => ({ kind: 'moveToRole', role: 'archive' }),
    summary: (action) =>
      action.kind === 'moveToRole'
        ? `Move to ${MOVE_ROLE_LABELS[action.role] ?? action.role}`
        : '',
  },
  markRead: {
    label: 'Mark read / unread',
    hint: 'Set the read state of the matched message.',
    defaultAction: () => ({ kind: 'markRead', read: true }),
    summary: (action) =>
      action.kind === 'markRead'
        ? action.read
          ? 'Mark as read'
          : 'Mark as unread'
        : '',
  },
  flag: {
    label: 'Flag / unflag',
    hint: 'Set the flagged state of the matched message.',
    defaultAction: () => ({ kind: 'flag', flagged: true }),
    summary: (action) =>
      action.kind === 'flag' ? (action.flagged ? 'Flag' : 'Unflag') : '',
  },
  notify: {
    label: 'Notify',
    hint: 'Raise an in-app notification (no external call).',
    defaultAction: () => ({ kind: 'notify', title: '', body: '' }),
    summary: (action) =>
      action.kind === 'notify' ? `Notify: ${action.title || '…'}` : '',
  },
  destroy: {
    label: 'Delete permanently (destroy)',
    hint: 'Permanently and unrecoverably delete the matched message. This is not a move to Trash.',
    defaultAction: () => ({ kind: 'destroy' }),
    summary: () => 'Delete permanently (unrecoverable)',
    destructive: true,
  },
  emit: {
    label: 'Emit a fact',
    hint: 'Emit rule.fired only — a client-side watcher decides what to do.',
    defaultAction: () => ({ kind: 'emit' }),
    summary: () => 'Emit rule.fired',
  },
  webhook: {
    label: 'Call a webhook',
    hint: 'POST the message + a scoped token to a URL.',
    defaultAction: () => ({
      // Least-grant default (threat 2 / ruling 23): read only.
      kind: 'webhook',
      url: '',
      grants: ['read'],
      expirySeconds: 3600,
    }),
    summary: (action) =>
      action.kind === 'webhook' ? `Webhook → ${action.url || '…'}` : '',
  },
}

/** Picker options, in the registry's declaration order. */
export const ACTION_KIND_OPTIONS = (
  Object.keys(ACTION_REGISTRY) as ActionKind[]
).map((kind) => ({ kind, ...ACTION_REGISTRY[kind] }))

/** The safe default when switching to a given action kind. */
export function defaultActionForKind(kind: ActionKind): WritableRuleAction {
  return ACTION_REGISTRY[kind].defaultAction()
}

/** Whether an action kind irreversibly destroys mail. */
export function isDestructiveActionKind(kind: string): boolean {
  return (
    kind in ACTION_REGISTRY &&
    ACTION_REGISTRY[kind as ActionKind].destructive === true
  )
}

/** A one-line human summary of an action for the rule list. `exec` (read-only,
 *  authored in rules.toml — never a registry entry) is summarised here. */
export function actionSummary(
  action: WritableRuleAction | { kind: string },
): string {
  if (action.kind === 'exec') {
    return 'Exec (config file)'
  }
  if (action.kind in ACTION_REGISTRY) {
    return ACTION_REGISTRY[action.kind as ActionKind].summary(
      action as WritableRuleAction,
    )
  }
  return action.kind
}
