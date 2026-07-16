import { describe, expect, it } from 'bun:test'

import type { TagAppearance } from '../src/api/types'
import {
  classifyRename,
  deleteTagAcrossCarriers,
  dropTagAppearance,
  migrateTagAppearance,
  renameTagAcrossCarriers,
  type KeywordDelta,
  type TagCarrier,
  type TagMaintenanceDeps,
} from '../src/components/settings-panel/tagMaintenance'

interface RecordedCall {
  carrier: string
  delta: KeywordDelta
}

function recorder(options: { failOn?: (carrier: TagCarrier) => boolean } = {}) {
  const calls: RecordedCall[] = []
  const applyKeywords = async (carrier: TagCarrier, delta: KeywordDelta) => {
    calls.push({ carrier: carrier.messageId, delta })
    if (options.failOn?.(carrier)) {
      throw new Error('boom')
    }
  }
  return { calls, applyKeywords }
}

function carriers(...ids: string[]): TagCarrier[] {
  return ids.map((id) => ({ sourceId: 's', messageId: id }))
}

describe('renameTagAcrossCarriers', () => {
  it('adds the new keyword before removing the old, per carrier', async () => {
    const { calls, applyKeywords } = recorder()
    const deps: TagMaintenanceDeps = {
      enumerateCarriers: async () => carriers('m1', 'm2'),
      applyKeywords,
      concurrency: 1,
    }

    const result = await renameTagAcrossCarriers('old', 'new', deps)

    expect(result).toEqual({ total: 2, failures: [] })
    expect(calls).toEqual([
      { carrier: 'm1', delta: { add: ['new'], remove: [] } },
      { carrier: 'm1', delta: { add: [], remove: ['old'] } },
      { carrier: 'm2', delta: { add: ['new'], remove: [] } },
      { carrier: 'm2', delta: { add: [], remove: ['old'] } },
    ])
    // The add for each carrier strictly precedes its remove.
    for (const id of ['m1', 'm2']) {
      const addIndex = calls.findIndex(
        (call) => call.carrier === id && call.delta.add.length > 0,
      )
      const removeIndex = calls.findIndex(
        (call) => call.carrier === id && call.delta.remove.length > 0,
      )
      expect(addIndex).toBeLessThan(removeIndex)
    }
  })

  it('records a carrier whose mutation fails, and never removes without adding', async () => {
    const { calls, applyKeywords } = recorder({
      // Fail the ADD of m2 — its remove must then be skipped (membership kept).
      failOn: (carrier) => carrier.messageId === 'm2',
    })
    const deps: TagMaintenanceDeps = {
      enumerateCarriers: async () => carriers('m1', 'm2'),
      applyKeywords,
      concurrency: 1,
    }

    const result = await renameTagAcrossCarriers('old', 'new', deps)

    expect(result.total).toBe(2)
    expect(result.failures.map((carrier) => carrier.messageId)).toEqual(['m2'])
    // m2 only ever saw the (failed) add — no remove was attempted.
    const m2Calls = calls.filter((call) => call.carrier === 'm2')
    expect(m2Calls).toEqual([
      { carrier: 'm2', delta: { add: ['new'], remove: [] } },
    ])
  })
})

describe('deleteTagAcrossCarriers', () => {
  it('strips the keyword from every carrier', async () => {
    const { calls, applyKeywords } = recorder()
    const deps: TagMaintenanceDeps = {
      enumerateCarriers: async () => carriers('m1', 'm2', 'm3'),
      applyKeywords,
      concurrency: 1,
    }

    const result = await deleteTagAcrossCarriers('spam', deps)

    expect(result).toEqual({ total: 3, failures: [] })
    expect(calls).toEqual([
      { carrier: 'm1', delta: { add: [], remove: ['spam'] } },
      { carrier: 'm2', delta: { add: [], remove: ['spam'] } },
      { carrier: 'm3', delta: { add: [], remove: ['spam'] } },
    ])
  })

  it('surfaces partial failures while completing the rest', async () => {
    const { applyKeywords } = recorder({
      failOn: (carrier) => carrier.messageId === 'm2',
    })
    const progress: Array<[number, number]> = []
    const deps: TagMaintenanceDeps = {
      enumerateCarriers: async () => carriers('m1', 'm2', 'm3'),
      applyKeywords,
      concurrency: 1,
      onProgress: (done, total) => progress.push([done, total]),
    }

    const result = await deleteTagAcrossCarriers('spam', deps)

    expect(result.total).toBe(3)
    expect(result.failures.map((carrier) => carrier.messageId)).toEqual(['m2'])
    // Progress reported once per carrier through to completion.
    expect(progress).toEqual([
      [1, 3],
      [2, 3],
      [3, 3],
    ])
  })

  it('caps concurrency at the requested in-flight limit', async () => {
    let inFlight = 0
    let peak = 0
    const deps: TagMaintenanceDeps = {
      enumerateCarriers: async () => carriers('a', 'b', 'c', 'd', 'e', 'f'),
      concurrency: 2,
      applyKeywords: async () => {
        inFlight += 1
        peak = Math.max(peak, inFlight)
        await Promise.resolve()
        inFlight -= 1
      },
    }

    await deleteTagAcrossCarriers('x', deps)
    expect(peak).toBeLessThanOrEqual(2)
  })
})

describe('classifyRename', () => {
  it('is a noop when the name is unchanged (case-insensitive)', () => {
    expect(classifyRename('Work', 'work', ['Work', 'Home'])).toBe('noop')
    expect(classifyRename('Work', '  ', ['Work'])).toBe('noop')
  })

  it('is a merge when the destination already exists as a tag', () => {
    expect(classifyRename('draft', 'Work', ['Work', 'draft'])).toBe('merge')
  })

  it('is a plain rename when the destination is new', () => {
    expect(classifyRename('draft', 'Projects', ['Work', 'draft'])).toBe(
      'rename',
    )
  })
})

describe('migrateTagAppearance', () => {
  const configured: TagAppearance[] = [
    { name: 'old', fg: '#111', bg: '#eee' },
    { name: 'other', icon: 'briefcase' },
  ]

  it('transfers the appearance entry to the new name (no collision)', () => {
    expect(migrateTagAppearance(configured, 'old', 'new')).toEqual([
      { name: 'new', fg: '#111', bg: '#eee' },
      { name: 'other', icon: 'briefcase' },
    ])
  })

  it('keeps the destination appearance and drops the source on merge', () => {
    const withDest: TagAppearance[] = [
      ...configured,
      { name: 'dest', fg: '#abc' },
    ]
    expect(migrateTagAppearance(withDest, 'old', 'dest')).toEqual([
      { name: 'other', icon: 'briefcase' },
      { name: 'dest', fg: '#abc' },
    ])
  })

  it('returns null when there is no entry to migrate', () => {
    expect(migrateTagAppearance(configured, 'absent', 'new')).toBeNull()
  })
})

describe('dropTagAppearance', () => {
  it('removes the entry for the deleted tag', () => {
    const configured: TagAppearance[] = [
      { name: 'old', fg: '#111' },
      { name: 'keep', icon: 'briefcase' },
    ]
    expect(dropTagAppearance(configured, 'old')).toEqual([
      { name: 'keep', icon: 'briefcase' },
    ])
  })

  it('returns null when the tag has no appearance entry', () => {
    expect(dropTagAppearance([{ name: 'keep' }], 'old')).toBeNull()
  })
})
