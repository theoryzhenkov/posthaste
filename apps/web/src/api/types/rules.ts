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

/** The safe action variants a GUI-created rule may carry. NO `exec`. */
export type WritableRuleAction =
  | { kind: 'tag'; tag: string }
  | { kind: 'move'; mailboxId: string }
  | { kind: 'notify'; title: string; body?: string | null }
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
}

/** Whether an action is the read-only, GUI-uneditable `exec` variant (authored
 *  in rules.toml). Used to render authored exec rules as locked. */
export function isExecAction(action: RuleAction): boolean {
  return action.kind === 'exec'
}
