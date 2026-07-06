import { describe, expect, it } from 'bun:test'

import type { MailboxGroup, SmartMailboxSummary } from '../src/api/types'
import {
  partitionSmartMailboxes,
  smartAssignableGroups,
  visibleSmartMailboxes,
} from '../src/components/sidebar/model'

const mkSmart = (id: string): SmartMailboxSummary => ({
  id,
  name: id,
  kind: 'user',
  defaultKey: null,
  role: null,
  parentId: null,
  unreadMessages: 0,
  totalMessages: 0,
  createdAt: '',
  updatedAt: '',
})

const smartMailboxes = [
  mkSmart('all'),
  mkSmart('flagged'),
  mkSmart('unread'),
  mkSmart('vip'),
]

const group = (
  id: string,
  order: number,
  mailboxIds: string[],
  name = id,
): MailboxGroup => ({ id, name, mailboxIds, order })

describe('partitionSmartMailboxes', () => {
  it('splits the smart section into ungrouped + Groups (grouped + ungrouped render)', () => {
    const groups = [group('g-triage', 0, ['flagged', 'unread'])]
    const { ungrouped, groups: rendered } = partitionSmartMailboxes(
      smartMailboxes,
      groups,
    )
    expect(ungrouped.map((m) => m.id)).toEqual(['all', 'vip'])
    expect(rendered).toHaveLength(1)
    expect(rendered[0]?.group.id).toBe('g-triage')
    // Members follow the section's own smart-mailbox order.
    expect(rendered[0]?.mailboxes.map((m) => m.id)).toEqual([
      'flagged',
      'unread',
    ])
  })

  it('orders Groups by `order`, ties broken by name', () => {
    const groups = [
      group('g-b', 1, ['unread'], 'B'),
      group('g-a', 0, ['flagged'], 'A'),
    ]
    const { groups: rendered } = partitionSmartMailboxes(smartMailboxes, groups)
    expect(rendered.map((entry) => entry.group.id)).toEqual(['g-a', 'g-b'])
  })

  it('drops a group with no smart members (e.g. a source group) from this section', () => {
    // A group holding only source-mailbox ids contributes nothing here — that
    // keeps a source group out of the Smart section.
    const groups = [group('g-source', 0, ['source-inbox', 'source-archive'])]
    const { ungrouped, groups: rendered } = partitionSmartMailboxes(
      smartMailboxes,
      groups,
    )
    expect(rendered).toHaveLength(0)
    expect(ungrouped).toHaveLength(smartMailboxes.length)
  })

  it('a stray mixed group shows only its smart members here', () => {
    const groups = [group('g-mixed', 0, ['flagged', 'source-inbox'])]
    const { groups: rendered } = partitionSmartMailboxes(smartMailboxes, groups)
    expect(rendered).toHaveLength(1)
    expect(rendered[0]?.mailboxes.map((m) => m.id)).toEqual(['flagged'])
  })

  it('a smart mailbox in no group is unaffected (all ungrouped when no groups)', () => {
    const { ungrouped, groups: rendered } = partitionSmartMailboxes(
      smartMailboxes,
      [],
    )
    expect(rendered).toHaveLength(0)
    expect(ungrouped.map((m) => m.id)).toEqual(smartMailboxes.map((m) => m.id))
  })
})

describe('visibleSmartMailboxes (j/k walk order)', () => {
  const groups = [group('g-triage', 0, ['flagged', 'unread'])]

  it('includes an expanded Group’s members in DOM order', () => {
    const visible = visibleSmartMailboxes(smartMailboxes, groups, new Set())
    expect(visible.map((m) => m.id)).toEqual([
      'all',
      'vip',
      'flagged',
      'unread',
    ])
  })

  it('a collapsed smart Group hides its members from the walk', () => {
    const visible = visibleSmartMailboxes(
      smartMailboxes,
      groups,
      new Set(['g-triage']),
    )
    expect(visible.map((m) => m.id)).toEqual(['all', 'vip'])
  })
})

describe('smartAssignableGroups (Add to group homogeneity)', () => {
  const smartIds = new Set(smartMailboxes.map((m) => m.id))

  it('offers a smart-only group but NOT a source-populated group', () => {
    const groups = [
      group('g-smart', 0, ['flagged']),
      group('g-source', 1, ['source-inbox']),
    ]
    const offered = smartAssignableGroups(groups, smartIds)
    expect(offered.map((g) => g.id)).toEqual(['g-smart'])
  })

  it('excludes a mixed group (would become non-homogeneous)', () => {
    const groups = [group('g-mixed', 0, ['flagged', 'source-inbox'])]
    expect(smartAssignableGroups(groups, smartIds)).toHaveLength(0)
  })
})
