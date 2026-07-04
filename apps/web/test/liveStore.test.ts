import { afterEach, describe, expect, it } from 'bun:test'

import {
  __resetLiveStoreForTesting,
  getConnectionHealth,
  getMailboxCounts,
  getViewProjection,
  setConnectionHealth,
  setMailboxCount,
  setViewProjection,
} from '../src/live-store/store'
import type { RuntimeMailListRowState } from '../src/runtime/types'

// The store is a module singleton — reset every slice + all listeners between
// cases so state can't bleed across tests.
afterEach(() => {
  __resetLiveStoreForTesting()
})

// A tiny external-store subscribe shim mirroring what useSyncExternalStore does:
// register a listener that bumps a version counter. The store's own subscribe is
// not exported (hooks own it), so we count notifications via a re-derived getter.
function row(id: string): RuntimeMailListRowState {
  return {
    rowKey: `s:${id}`,
    resourceRef: null,
    projection: { id, sourceId: 's' } as never,
    orderKey: id,
  } as RuntimeMailListRowState
}

describe('liveStore counts slice', () => {
  it('mirrors a count and reads it back keyed by account+mailbox', () => {
    setMailboxCount('acc', 'inbox', { unread: 3, total: 10 })
    expect(getMailboxCounts('acc').inbox).toEqual({ unread: 3, total: 10 })
  })

  it('returns a stable empty object for an account with no counts', () => {
    const a = getMailboxCounts('missing')
    const b = getMailboxCounts('missing')
    expect(a).toEqual({})
    expect(a).toBe(b) // same reference → no spurious re-render
  })

  it('keeps a stable snapshot until the value actually moves', () => {
    setMailboxCount('acc', 'inbox', { unread: 1, total: 5 })
    const first = getMailboxCounts('acc')
    // An identical write is a no-op: the reference must not change.
    setMailboxCount('acc', 'inbox', { unread: 1, total: 5 })
    expect(getMailboxCounts('acc')).toBe(first)
    // A real change mints a new reference for THIS account.
    setMailboxCount('acc', 'inbox', { unread: 0, total: 5 })
    expect(getMailboxCounts('acc')).not.toBe(first)
    expect(getMailboxCounts('acc').inbox).toEqual({ unread: 0, total: 5 })
  })

  it('isolates slices — an unrelated account keeps its reference', () => {
    setMailboxCount('a', 'inbox', { unread: 1, total: 1 })
    const aBefore = getMailboxCounts('a')
    setMailboxCount('b', 'inbox', { unread: 2, total: 2 })
    // Writing account b must not disturb account a's snapshot.
    expect(getMailboxCounts('a')).toBe(aBefore)
  })
})

describe('liveStore view slice', () => {
  it('mirrors projected rows and returns a stable empty array when absent', () => {
    const empty = getViewProjection('v-none')
    expect(empty).toEqual([])
    expect(getViewProjection('v-none')).toBe(empty)

    const rows = [row('m1'), row('m2')]
    setViewProjection('v1', rows)
    expect(getViewProjection('v1')).toBe(rows)
  })

  it('isolates the view slice from the counts slice', () => {
    const rows = [row('m1')]
    setViewProjection('v1', rows)
    // A counts write must not replace the view slice's reference.
    setMailboxCount('acc', 'inbox', { unread: 1, total: 1 })
    expect(getViewProjection('v1')).toBe(rows)
  })
})

describe('liveStore connection health', () => {
  it('defaults to healthy and updates on set', () => {
    expect(getConnectionHealth()).toBe('healthy')
    setConnectionHealth('degraded')
    expect(getConnectionHealth()).toBe('degraded')
  })
})
