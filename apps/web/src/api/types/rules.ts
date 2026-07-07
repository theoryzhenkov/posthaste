/**
 * Automation-rule types (RFC-L2-scripting ruling 23). Mirrors the generated
 * `components["schemas"]` for `Rule` / `RuleAction` / `RuleGrant` /
 * `WritableRuleAction` / `WritableRuleInput`, in the hand-authored domain-type
 * layer the panes import.
 *
 * The distinction that carries the security invariant: {@link RuleAction} (READ
 * side) includes `exec`; {@link WritableRuleAction} (WRITE side) does NOT — exec
 * is config-file-only, unrepresentable over the GUI/REST write surface. The
 * action picker builds a `WritableRuleAction`, so it *cannot* express exec.
 *
 * @spec docs/eph/RFC-L2-scripting#7-rulings
 */
import type { SmartMailboxRule } from './smartMailboxes'

/** A capability a webhook token carries — least-privilege verbs only. */
export type RuleGrant = 'read' | 'send' | 'tag' | 'move' | 'delete'

/**
 * The mailbox roles a `moveToRole` rule action may target — the server's
 * `ASSIGNABLE_RULE_MOVE_ROLES` subset (drafts/sent are provider-managed;
 * snooze needs the paired return time), NOT the full assignable-role set the
 * mailbox editor uses.
 */
export const RULE_MOVE_ROLES = ['archive', 'junk', 'trash', 'inbox'] as const
export type RuleMoveRole = (typeof RULE_MOVE_ROLES)[number]

/** The safe action variants a GUI-created rule may carry. NO `exec`. */
export type WritableRuleAction =
  | { kind: 'tag'; tag: string }
  | { kind: 'move'; mailboxId: string }
  | { kind: 'moveToRole'; role: RuleMoveRole }
  | { kind: 'markRead'; read: boolean }
  | { kind: 'flag'; flagged: boolean }
  | { kind: 'notify'; title: string; body?: string | null }
  /** Mail-DESTRUCTIVE: permanently deletes the matched message (not a move to
   *  Trash). The server refuses it unless the WHEN-clause has ≥1 condition. */
  | { kind: 'destroy' }
  | { kind: 'emit' }
  | {
      kind: 'webhook'
      url: string
      grants?: RuleGrant[]
      expirySeconds?: number
    }

/** The full action enum returned by the read surface — includes `exec`, which
 *  the GUI renders read-only (an authored rule from rules.toml). */
export type RuleAction =
  | WritableRuleAction
  | {
      kind: 'exec'
      command: string
      grants?: RuleGrant[]
      expirySeconds?: number
    }

/** A rule as returned by `GET /v1/rules` (the merged ruleset). */
export interface Rule {
  id: string
  name: string
  when: SmartMailboxRule
  /** Trigger topics; empty ⇒ the message-update default family. */
  on?: string[]
  action: RuleAction
  enabled: boolean
  /** Rule chaining: when this rule matches a fact, later rules are skipped
   *  for that fact. Rules run in order — authored (rules.toml) first in file
   *  order, then GUI rules sorted by name. */
  stopProcessing?: boolean
}

/** `GET /v1/rules` response. */
export interface RulesListResponse {
  rules: Rule[]
}

/** The create/replace body (`POST`/`PUT /v1/rules`). `action` is a
 *  {@link WritableRuleAction}, so exec is not expressible. */
export interface WritableRuleInput {
  id?: string
  name: string
  when: SmartMailboxRule
  on?: string[]
  enabled?: boolean
  action: WritableRuleAction
  /** See {@link Rule.stopProcessing}. Defaults to false. */
  stopProcessing?: boolean
}

/** Whether an action is the read-only, GUI-uneditable `exec` variant (authored
 *  in rules.toml). Used to render authored exec rules as locked. */
export function isExecAction(action: RuleAction): boolean {
  return action.kind === 'exec'
}
