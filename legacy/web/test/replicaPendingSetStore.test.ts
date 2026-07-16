import { describe, expect, it } from 'bun:test'

import { MemoryPendingSetStore } from '../src/runtime/replica/pendingSetStore'
import type { PendingSetRecord } from '../src/runtime/replica/pendingSetStore'

function record(id: string, acceptedAt: number): PendingSetRecord {
  return {
    clientMutationId: id,
    messageId: `m-${id}`,
    assertion: { kind: 'setKeywords', add: ['$seen'], remove: [] },
    runtimeMutationId: null,
    acceptedAt,
  }
}

describe('pending set store semantics', () => {
  it('persists, links runtime ids, and removes by client mutation id', async () => {
    const store = new MemoryPendingSetStore()
    await store.put(record('c1', 1))
    await store.put(record('c2', 2))

    await store.linkRuntimeMutationId('c1', 'r1')

    const all = await store.all()
    expect(all.map((r) => r.clientMutationId)).toEqual(['c1', 'c2'])
    expect(all[0]?.runtimeMutationId).toBe('r1')

    await store.remove('c1')
    expect((await store.all()).map((r) => r.clientMutationId)).toEqual(['c2'])
  })

  it('is idempotent on put and orders replay by acceptance time', async () => {
    const store = new MemoryPendingSetStore()
    await store.put(record('b', 20))
    await store.put(record('a', 10))
    await store.put(record('a', 10)) // re-accept: overwrite, not duplicate

    expect((await store.all()).map((r) => r.clientMutationId)).toEqual([
      'a',
      'b',
    ])
  })

  it('ignores a runtime-id link for an unknown mutation', async () => {
    const store = new MemoryPendingSetStore()
    await store.linkRuntimeMutationId('missing', 'r9')
    expect(await store.all()).toEqual([])
  })
})
