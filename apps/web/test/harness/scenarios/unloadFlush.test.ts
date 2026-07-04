/**
 * Scenario (c) — W3 / N18: a page-unload flush can't strand an in-flight
 * durable write.
 *
 * An incoming frame enqueues its store op fire-and-forget (`void enqueue(...)`);
 * nothing in the caller awaits it. On `visibilitychange → hidden` / `pagehide`
 * the unload-durability hook awaits `flushActiveEntityStore()` so the queued op
 * (and any durable pending-set write inside it) completes before teardown. This
 * re-expresses the adapter suite's W3 test through the harness AND exercises the
 * real DOM wiring in `unloadDurability.ts`.
 *
 * @spec docs/eph/RFC-L2-client-resilience.md (W3 / N18)
 */
import { afterEach, describe, expect, it } from 'bun:test'

import { setupDomEnvironment } from '../../dom-env'
import { createClientHarness, messageUpdatedFrame } from '../index'
import { flushActiveEntityStore } from '../../../src/runtime/replica/entityStoreAdapter'
import {
  installUnloadDurabilityHooks,
  resetUnloadDurabilityHooksForTesting,
} from '../../../src/runtime/replica/unloadDurability'
import type {
  RuntimeFrame,
  RuntimeMailListViewState,
} from '../../../src/runtime/types'

setupDomEnvironment()

afterEach(() => {
  resetUnloadDurabilityHooksForTesting()
})

const flagged = (id: string, receivedAt: string) =>
  messageUpdatedFrame(
    id,
    {
      id,
      sourceId: 's',
      receivedAt,
      keywords: ['$flagged'],
      mailboxIds: ['inbox'],
      isRead: false,
      isFlagged: true,
      subject: id,
    },
    [{ mailboxId: 'inbox', unreadCount: 2, totalCount: 2 }],
  )

function reprojected(
  frames: RuntimeFrame<RuntimeMailListViewState>[],
): boolean {
  return frames.some((f) => f.type === 'viewReplace')
}

describe('scenario W3: unload flush drains an in-flight store op', () => {
  it('flushActiveEntityStore awaits the fire-and-forget re-projection', async () => {
    const h = await createClientHarness({ coalescer: 'synchronous' })
    await h.openView()

    h.emitFrame(flagged('m1', '2026-04-29T10:00:00Z'))
    // The queued re-projection hasn't run yet — the race a bare tab-close lands in.
    expect(reprojected(h.frames)).toBe(false)

    await flushActiveEntityStore()

    // The flush guarantees the queued op completed before returning.
    expect(reprojected(h.frames)).toBe(true)
    h.dispose()
  })

  it('the visibilitychange→hidden hook drains the queued op (real DOM wiring)', async () => {
    installUnloadDurabilityHooks()
    const h = await createClientHarness({ coalescer: 'synchronous' })
    await h.openView()

    h.emitFrame(flagged('m1', '2026-04-29T10:00:00Z'))
    expect(reprojected(h.frames)).toBe(false)

    // Fire the real lifecycle event the hook listens for.
    Object.defineProperty(document, 'visibilityState', {
      value: 'hidden',
      configurable: true,
    })
    document.dispatchEvent(new Event('visibilitychange'))

    // Let the hook's awaited flush settle.
    await flushActiveEntityStore()
    await new Promise<void>((resolve) => setTimeout(resolve, 0))

    expect(reprojected(h.frames)).toBe(true)
    h.dispose()
  })
})
