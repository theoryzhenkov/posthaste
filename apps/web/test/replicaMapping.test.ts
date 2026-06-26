import { describe, expect, it } from 'bun:test'

import {
  membershipMailbox,
  settlementVerdict,
} from '../src/runtime/replica/mapping'

describe('replica mapping', () => {
  it('yields a concrete mailbox only for a single source mailbox', () => {
    expect(
      membershipMailbox({
        kind: 'source-mailbox',
        sourceId: 's',
        mailboxId: 'inbox',
      }),
    ).toBe('inbox')
    expect(
      membershipMailbox({
        kind: 'source-mailbox',
        sourceId: 's',
        mailboxId: null,
      }),
    ).toBeNull()
    expect(
      membershipMailbox({ kind: 'smart-mailbox', smartMailboxId: 'sm' }),
    ).toBeNull()
    expect(membershipMailbox({ kind: 'global' })).toBeNull()
  })

  it('maps settlement statuses to verdicts, ignoring non-terminal ones', () => {
    expect(settlementVerdict('confirmed')).toBe('confirmed')
    expect(settlementVerdict('failed')).toBe('failed')
    expect(settlementVerdict('conflict')).toBe('failed')
    expect(settlementVerdict('accepted')).toBeNull()
    expect(settlementVerdict('localApplied')).toBeNull()
    expect(settlementVerdict('queued')).toBeNull()
  })
})
