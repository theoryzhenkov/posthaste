import { describe, expect, it } from 'bun:test'

import {
  resolveGotoTarget,
  stepGotoPrefix,
  type GotoPrefix,
} from '../src/components/keyboard/goto'

const SMART = [
  {
    id: 'default-inbox',
    name: 'Inbox',
    role: 'inbox',
    kind: 'default',
    position: 0,
  },
  {
    id: 'default-archive',
    name: 'Archive',
    role: 'archive',
    kind: 'default',
    position: 1,
  },
  {
    id: 'default-trash',
    name: 'Trash',
    role: 'trash',
    kind: 'default',
    position: 5,
  },
  // A user smart mailbox that also claims the inbox role, higher position.
  {
    id: 'user-inbox',
    name: 'My Inbox',
    role: 'inbox',
    kind: 'user',
    position: 9,
  },
]

describe('stepGotoPrefix', () => {
  it('maps the context-aware g prefix to inbox/archive/trash', () => {
    expect(stepGotoPrefix('g', 'i')).toEqual({
      type: 'goto',
      role: 'inbox',
      forceSmart: false,
    })
    expect(stepGotoPrefix('g', 'a')).toEqual({
      type: 'goto',
      role: 'archive',
      forceSmart: false,
    })
    expect(stepGotoPrefix('g', 't')).toEqual({
      type: 'goto',
      role: 'trash',
      forceSmart: false,
    })
  })

  it('escalates g q to the force-smart sub-prefix', () => {
    expect(stepGotoPrefix('g', 'q')).toEqual({ type: 'await-q' })
  })

  it('maps every role under the gq prefix, force-smart', () => {
    const cases: [string, string][] = [
      ['i', 'inbox'],
      ['a', 'archive'],
      ['t', 'trash'],
      ['d', 'drafts'],
      ['s', 'sent'],
      ['j', 'junk'],
    ]
    for (const [key, role] of cases) {
      expect(stepGotoPrefix('gq', key)).toEqual({
        type: 'goto',
        role: role as never,
        forceSmart: true,
      })
    }
  })

  it('cancels on unmapped keys (drafts/sent are gq-only)', () => {
    expect(stepGotoPrefix('g', 'd')).toEqual({ type: 'cancel' })
    expect(stepGotoPrefix('g', 'x')).toEqual({ type: 'cancel' })
    expect(stepGotoPrefix('gq', 'x')).toEqual({ type: 'cancel' })
    expect(stepGotoPrefix(null as GotoPrefix, 'i')).toEqual({ type: 'cancel' })
  })
})

describe('resolveGotoTarget', () => {
  it('jumps to the same account folder from a source-mailbox view', () => {
    const target = resolveGotoTarget({
      effectiveView: {
        kind: 'source-mailbox',
        sourceId: 'acct-1',
        mailboxId: 'mbx-current',
      },
      role: 'archive',
      forceSmart: false,
      sourceMailboxes: [
        { id: 'mbx-arch', name: 'Archive', role: 'archive' },
        { id: 'mbx-current', name: 'Inbox', role: 'inbox' },
      ],
      smartMailboxes: SMART,
    })
    expect(target).toEqual({
      kind: 'source-mailbox',
      sourceId: 'acct-1',
      mailboxId: 'mbx-arch',
      mailboxName: 'Archive',
    })
  })

  it('falls back to the smart mailbox when the account lacks the folder', () => {
    const target = resolveGotoTarget({
      effectiveView: {
        kind: 'source-mailbox',
        sourceId: 'acct-1',
        mailboxId: 'mbx-current',
      },
      role: 'trash',
      forceSmart: false,
      sourceMailboxes: [{ id: 'mbx-current', name: 'Inbox', role: 'inbox' }],
      smartMailboxes: SMART,
    })
    expect(target).toEqual({
      kind: 'smart-mailbox',
      id: 'default-trash',
      name: 'Trash',
    })
  })

  it('always uses the smart mailbox in a smart-mailbox view', () => {
    const target = resolveGotoTarget({
      effectiveView: { kind: 'smart-mailbox', id: 'default-archive' },
      role: 'inbox',
      forceSmart: false,
      sourceMailboxes: [],
      smartMailboxes: SMART,
    })
    expect(target).toEqual({
      kind: 'smart-mailbox',
      id: 'default-inbox',
      name: 'Inbox',
    })
  })

  it('forceSmart ignores source-mailbox context entirely', () => {
    const target = resolveGotoTarget({
      effectiveView: {
        kind: 'source-mailbox',
        sourceId: 'acct-1',
        mailboxId: 'mbx-current',
      },
      role: 'inbox',
      forceSmart: true,
      sourceMailboxes: [{ id: 'mbx-inbox', name: 'Inbox', role: 'inbox' }],
      smartMailboxes: SMART,
    })
    expect(target).toEqual({
      kind: 'smart-mailbox',
      id: 'default-inbox',
      name: 'Inbox',
    })
  })

  it('prefers the built-in default over a user smart mailbox for the same role', () => {
    const target = resolveGotoTarget({
      effectiveView: null,
      role: 'inbox',
      forceSmart: true,
      sourceMailboxes: [],
      smartMailboxes: [...SMART].reverse(),
    })
    expect(target).toEqual({
      kind: 'smart-mailbox',
      id: 'default-inbox',
      name: 'Inbox',
    })
  })

  it('returns null when no mailbox carries the role', () => {
    expect(
      resolveGotoTarget({
        effectiveView: null,
        role: 'junk',
        forceSmart: true,
        sourceMailboxes: [],
        smartMailboxes: SMART,
      }),
    ).toBeNull()
  })
})
