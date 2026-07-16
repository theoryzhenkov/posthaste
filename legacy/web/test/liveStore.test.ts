import { afterEach, describe, expect, it } from 'bun:test'

import {
  __resetLiveStoreForTesting,
  getConnectionHealth,
  getViewProjection,
  setConnectionHealth,
  setViewProjection,
} from '../src/live-store/store'
import type { RuntimeMailListRowState } from '../src/runtime/types'

// The store is a module singleton — reset every slice + all listeners between
// cases so state can't bleed across tests.
afterEach(() => {
  __resetLiveStoreForTesting()
})

function row(id: string): RuntimeMailListRowState {
  return {
    rowKey: `s:${id}`,
    resourceRef: null,
    projection: { id, sourceId: 's' } as never,
    orderKey: id,
  } as RuntimeMailListRowState
}

// Mailbox counts no longer live in the live store (RFC-L2-count-unification):
// they are react-query state (`domain-cache/mailboxCounts.ts` owns the
// invalidation + overlay; see mailboxCounts.test.ts). Only the view projection
// and connection-health slices remain here.

describe('liveStore view slice', () => {
  it('mirrors projected rows and returns a stable empty array when absent', () => {
    const empty = getViewProjection('v-none')
    expect(empty).toEqual([])
    expect(getViewProjection('v-none')).toBe(empty)

    const rows = [row('m1'), row('m2')]
    setViewProjection('v1', rows)
    expect(getViewProjection('v1')).toBe(rows)
  })

  it('keeps a stable reference until the rows actually change', () => {
    const rows = [row('m1')]
    setViewProjection('v1', rows)
    // An identical write is a no-op: the reference must not change.
    setViewProjection('v1', rows)
    expect(getViewProjection('v1')).toBe(rows)

    const next = [row('m1'), row('m2')]
    setViewProjection('v1', next)
    expect(getViewProjection('v1')).toBe(next)
  })

  it('isolates views — writing one view keeps another view untouched', () => {
    const rows = [row('m1')]
    setViewProjection('v1', rows)
    setViewProjection('v2', [row('m2')])
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
