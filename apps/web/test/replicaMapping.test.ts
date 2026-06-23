import { describe, expect, it } from 'bun:test'

import type {
  RuntimeMailListRowState,
  RuntimeMailListViewState,
} from '../src/runtime/types'
import {
  membershipMailbox,
  messageIdForRow,
  replicaRowsFromViewState,
  settlementVerdict,
} from '../src/runtime/replica/mapping'

function row(
  resourceRef: string | null,
  projection: Record<string, unknown>,
): RuntimeMailListRowState {
  return {
    rowKey: (projection.id as string) ?? 'k',
    resourceRef,
    projection: projection as RuntimeMailListRowState['projection'],
    orderKey: '0',
  }
}

describe('replica mapping', () => {
  it('derives the bare message id from a resourceRef', () => {
    expect(messageIdForRow(row('message:src-1:abc', { id: 'abc' }))).toBe('abc')
  })

  it('falls back to the projection id without a usable ref', () => {
    expect(messageIdForRow(row(null, { id: 'xyz' }))).toBe('xyz')
    expect(messageIdForRow(row('conversation:1', { id: 'xyz' }))).toBe('xyz')
  })

  it('maps a served view state to replica rows in order', () => {
    const state = {
      rows: [
        row('message:s:1', { id: '1', subject: 'a' }),
        row('message:s:2', { id: '2', subject: 'b' }),
      ],
    } as unknown as RuntimeMailListViewState

    expect(replicaRowsFromViewState(state)).toEqual([
      { messageId: '1', projection: { id: '1', subject: 'a' } },
      { messageId: '2', projection: { id: '2', subject: 'b' } },
    ])
  })

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
