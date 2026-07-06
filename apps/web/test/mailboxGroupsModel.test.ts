import { describe, expect, it } from 'bun:test'

import type { Mailbox, MailboxGroup } from '../src/api/types'
import {
  partitionSourceMailboxes,
  visibleSourceMailboxes,
} from '../src/components/sidebar/model'

const mkMailbox = (id: string): Mailbox => ({
  id,
  name: id,
  role: null,
  unreadEmails: 0,
  totalEmails: 0,
})

const mailboxes = [
  mkMailbox('inbox'),
  mkMailbox('receipts'),
  mkMailbox('travel'),
  mkMailbox('archive'),
]

const group = (
  id: string,
  order: number,
  mailboxIds: string[],
  name = id,
): MailboxGroup => ({ id, name, mailboxIds, order })

describe('partitionSourceMailboxes', () => {
  it('splits a source into ungrouped mailboxes + Groups (grouped + ungrouped render)', () => {
    const groups = [group('g-finance', 0, ['receipts', 'travel'])]
    const { ungrouped, groups: rendered } = partitionSourceMailboxes(
      mailboxes,
      groups,
    )
    expect(ungrouped.map((m) => m.id)).toEqual(['inbox', 'archive'])
    expect(rendered).toHaveLength(1)
    expect(rendered[0]?.group.id).toBe('g-finance')
    // Members follow the source's own mailbox order, not id-assignment order.
    expect(rendered[0]?.mailboxes.map((m) => m.id)).toEqual([
      'receipts',
      'travel',
    ])
  })

  it('orders Groups by `order`, ties broken by name', () => {
    const groups = [
      group('g-b', 1, ['travel'], 'B'),
      group('g-a', 0, ['receipts'], 'A'),
    ]
    const { groups: rendered } = partitionSourceMailboxes(mailboxes, groups)
    expect(rendered.map((entry) => entry.group.id)).toEqual(['g-a', 'g-b'])
  })

  it('surfaces a Group only when it holds ≥1 of THIS source’s mailboxes', () => {
    // A group whose members belong to some other source contributes nothing.
    const groups = [group('g-other', 0, ['other-source-mbx'])]
    const { ungrouped, groups: rendered } = partitionSourceMailboxes(
      mailboxes,
      groups,
    )
    expect(rendered).toHaveLength(0)
    expect(ungrouped).toHaveLength(mailboxes.length)
  })

  it('a mailbox in no group is unaffected (all ungrouped when no groups)', () => {
    const { ungrouped, groups: rendered } = partitionSourceMailboxes(
      mailboxes,
      [],
    )
    expect(rendered).toHaveLength(0)
    expect(ungrouped.map((m) => m.id)).toEqual(mailboxes.map((m) => m.id))
  })
})

describe('visibleSourceMailboxes (j/k walk order)', () => {
  const groups = [group('g-finance', 0, ['receipts', 'travel'])]

  it('includes an expanded Group’s members in DOM order', () => {
    const visible = visibleSourceMailboxes(mailboxes, groups, new Set())
    expect(visible.map((m) => m.id)).toEqual([
      'inbox',
      'archive',
      'receipts',
      'travel',
    ])
  })

  it('a collapsed Group hides its members from the walk', () => {
    const visible = visibleSourceMailboxes(
      mailboxes,
      groups,
      new Set(['g-finance']),
    )
    expect(visible.map((m) => m.id)).toEqual(['inbox', 'archive'])
  })
})
