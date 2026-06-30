import { afterEach, describe, expect, it } from 'bun:test'

import { request } from '../src/api/client/core'
import {
  applyResolvedConnection,
  resetActiveConnectionForTesting,
} from '../src/connection/runtime'

const originalFetch = globalThis.fetch

afterEach(() => {
  globalThis.fetch = originalFetch
  resetActiveConnectionForTesting()
})

const jsonOk = () =>
  new Response('{}', {
    status: 200,
    headers: { 'content-type': 'application/json' },
  })

const tick = () => new Promise<void>((resolve) => setTimeout(resolve, 0))

describe('API request concurrency cap', () => {
  it('never runs more than the cap concurrently, and drains the overflow', async () => {
    applyResolvedConnection(
      { baseUrl: 'http://127.0.0.1:1/v1', token: 't' },
      null,
    )
    let active = 0
    let peak = 0
    let started = 0
    const releases: Array<() => void> = []
    globalThis.fetch = (async () => {
      started += 1
      active += 1
      peak = Math.max(peak, active)
      await new Promise<void>((resolve) => releases.push(resolve))
      active -= 1
      return jsonOk()
    }) as typeof fetch

    const count = 9
    const all = Array.from({ length: count }, () => request('/x'))
    await tick()
    // Only the cap's worth of fetches have started; the rest are queued.
    expect(started).toBe(4)
    expect(peak).toBe(4)

    // Releasing each in-flight fetch lets a queued request take the slot.
    while (releases.length > 0) {
      releases.shift()!()
      await tick()
    }
    await Promise.all(all)
    expect(started).toBe(count)
    expect(peak).toBe(4)
  })

  it('releases the slot when a request fails, so it cannot wedge the queue', async () => {
    applyResolvedConnection(
      { baseUrl: 'http://127.0.0.1:1/v1', token: 't' },
      null,
    )
    let calls = 0
    globalThis.fetch = (async () => {
      calls += 1
      // The first cap-worth of requests fail before a response.
      if (calls <= 4) throw new Error('network connection lost')
      return jsonOk()
    }) as typeof fetch

    // If a failed request leaked its slot, the 5th would wait forever.
    const settled = await Promise.allSettled(
      Array.from({ length: 5 }, () => request('/x')),
    )
    expect(settled.filter((s) => s.status === 'rejected')).toHaveLength(4)
    expect(settled[4]?.status).toBe('fulfilled')
  })
})
