/**
 * The rule-action registry (DESIGN-L2-rule-actions): the web's single source
 * for the writable action vocabulary. These pin
 *
 * * completeness — every writable wire kind has exactly one registry row whose
 *   default round-trips as that kind;
 * * the destructive labelling for `destroy` (unmistakable in the picker/list);
 * * the STRUCTURAL exec exclusion — the registry (and therefore the picker) is
 *   keyed by `WritableRuleAction['kind']`, which has no `exec` member, so a
 *   GUI-authored exec action is unrepresentable (the web half of the
 *   RCE-boundary invariant; the server halves live in
 *   `posthaste-domain-model::rules` and `rules_crud_e2e`).
 */
import { describe, expect, it } from 'bun:test'

import type { WritableRuleAction } from '../src/api/types'
import {
  ACTION_KIND_OPTIONS,
  ACTION_REGISTRY,
  actionSummary,
  defaultActionForKind,
  isDestructiveActionKind,
  type ActionKind,
} from '../src/components/settings-panel/automations/ruleActionHelpers'

/** The full writable vocabulary, kept in one place for the parity checks.
 *  (Type-level completeness: `satisfies` fails compilation if a union member
 *  is missing or invented.) */
const ALL_KINDS = [
  'tag',
  'move',
  'moveToRole',
  'markRead',
  'flag',
  'notify',
  'destroy',
  'emit',
  'webhook',
] as const satisfies readonly ActionKind[]

describe('rule action registry', () => {
  it('covers every writable kind with a default that round-trips its kind', () => {
    expect(Object.keys(ACTION_REGISTRY).sort()).toEqual([...ALL_KINDS].sort())
    for (const kind of ALL_KINDS) {
      const action = defaultActionForKind(kind)
      expect(action.kind).toBe(kind)
      expect(ACTION_REGISTRY[kind].label.length).toBeGreaterThan(0)
      expect(ACTION_REGISTRY[kind].hint.length).toBeGreaterThan(0)
      // Every configured action gets a non-empty one-line summary.
      expect(actionSummary(action).length).toBeGreaterThan(0)
    }
  })

  it('cannot represent exec — the structural GUI half of the RCE boundary', () => {
    // Not a picker option…
    expect(
      ACTION_KIND_OPTIONS.some((option) => (option.kind as string) === 'exec'),
    ).toBe(false)
    // …not a registry row…
    expect('exec' in ACTION_REGISTRY).toBe(false)
    // …and the type union itself excludes it (compile-time; asserted here for
    // the runtime record too).
    const kinds: readonly string[] = ALL_KINDS
    expect(kinds).not.toContain('exec')
    // The read-only summary for an authored exec rule labels it config-file.
    expect(actionSummary({ kind: 'exec' })).toBe('Exec (config file)')
  })

  it('marks destroy — and ONLY destroy — as destructive', () => {
    expect(ACTION_REGISTRY.destroy.destructive).toBe(true)
    expect(isDestructiveActionKind('destroy')).toBe(true)
    for (const kind of ALL_KINDS.filter((k) => k !== 'destroy')) {
      expect(isDestructiveActionKind(kind)).toBe(false)
    }
    // Unknown/read-only kinds are not destructive either.
    expect(isDestructiveActionKind('exec')).toBe(false)
  })

  it('destroy reads as permanent deletion, never as a move', () => {
    const summary = actionSummary({ kind: 'destroy' } as WritableRuleAction)
    expect(summary.toLowerCase()).toContain('permanent')
    expect(ACTION_REGISTRY.destroy.label.toLowerCase()).toContain('destroy')
    expect(ACTION_REGISTRY.destroy.hint.toLowerCase()).toContain(
      'not a move to trash',
    )
  })

  it('webhook default is least-grant (read only)', () => {
    const webhook = defaultActionForKind('webhook')
    expect(webhook).toEqual({
      kind: 'webhook',
      url: '',
      grants: ['read'],
      expirySeconds: 3600,
    })
  })
})
