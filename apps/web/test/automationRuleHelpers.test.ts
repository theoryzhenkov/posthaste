import { describe, expect, it } from 'bun:test'

import {
  actionForKind,
  actionListDescription,
  defaultDraft,
  isActionComplete,
  isDraftComplete,
  parseActionKind,
  parseTrigger,
  ruleActionSummary,
} from '../src/components/settings-panel/automationRuleHelpers'

describe('automation rule helpers', () => {
  // spec: docs/L1-api#application-settings
  it('builds the canonical empty action for each kind', () => {
    expect(actionForKind('applyTag')).toEqual({ kind: 'applyTag', tag: '' })
    expect(actionForKind('moveToMailbox')).toEqual({
      kind: 'moveToMailbox',
      mailboxId: '',
    })
    expect(actionForKind('markRead')).toEqual({ kind: 'markRead' })
  })

  it('treats tag/move actions as complete only when their target is filled', () => {
    expect(isActionComplete({ kind: 'applyTag', tag: 'work' })).toBe(true)
    expect(isActionComplete({ kind: 'applyTag', tag: '   ' })).toBe(false)
    // system-keyword tags ($-prefixed) are not user-assignable
    expect(isActionComplete({ kind: 'applyTag', tag: '$seen' })).toBe(false)
    expect(isActionComplete({ kind: 'moveToMailbox', mailboxId: 'mb1' })).toBe(
      true,
    )
    expect(isActionComplete({ kind: 'moveToMailbox', mailboxId: '' })).toBe(
      false,
    )
    // toggle actions are always complete
    expect(isActionComplete({ kind: 'flag' })).toBe(true)
  })

  it('defaultDraft is well-formed but incomplete until its action gets a target', () => {
    const draft = defaultDraft({ accountId: 'acct', name: 'Rule' })
    expect(draft.enabled).toBe(true)
    expect(draft.triggers).toEqual(['messageArrived'])
    expect(draft.backfill).toBe(true)
    expect(draft.actions).toHaveLength(1)
    // its lone action is an empty applyTag, so the draft is not yet complete
    expect(isDraftComplete(draft)).toBe(false)

    const ready = { ...draft, actions: [{ kind: 'flag' as const }] }
    expect(isDraftComplete(ready)).toBe(true)
    // missing account/name also blocks completion
    expect(isDraftComplete({ ...ready, accountId: '  ' })).toBe(false)
    expect(isDraftComplete({ ...ready, name: '' })).toBe(false)
  })

  it('parses trigger/action-kind values, falling back on unknown input', () => {
    expect(parseTrigger('manual', 'messageArrived')).toBe('manual')
    expect(parseTrigger('bogus', 'messageArrived')).toBe('messageArrived')
    expect(parseActionKind('flag', 'applyTag')).toBe('flag')
    expect(parseActionKind('nope', 'applyTag')).toBe('applyTag')
  })

  it('summarizes a rule by its first action, counting any extras', () => {
    expect(
      ruleActionSummary({
        ...defaultDraft({ accountId: 'a', name: 'n' }),
        actions: [{ kind: 'applyTag', tag: 'work' }],
      }),
    ).toBe('Tag work')
    expect(
      ruleActionSummary({
        ...defaultDraft({ accountId: 'a', name: 'n' }),
        actions: [{ kind: 'flag' }, { kind: 'markRead' }],
      }),
    ).toBe('Flag +1')
    expect(
      ruleActionSummary({
        ...defaultDraft({ accountId: 'a', name: 'n' }),
        actions: [],
      }),
    ).toBe('No actions')
  })

  it('describes the action list with rule/draft counts', () => {
    expect(actionListDescription([])).toBe('No actions configured.')
    const base = defaultDraft({ accountId: 'a', name: 'n' })
    expect(
      actionListDescription([
        { state: 'active', draft: base },
        { state: 'draft', draft: base },
      ]),
    ).toBe('2 actions in 2 rules. 1 draft.')
  })
})
