/**
 * Scenario (d) — D49: a stream gap (a seq gap the far-end could not replay)
 * arrives as a reset.
 *
 * Pins the CURRENT behavior as a baseline for M44 to change: on `onReset` the
 * adapter DROPS any buffered incremental `message.updated` frames (and cancels
 * the scheduled coalesced flush), then surfaces the reset upward. Recovery is
 * by the runtime re-serving whole snapshots over the fresh subscription — the
 * incremental deltas in flight at the gap are deliberately discarded. When M44
 * lands the reconcile pass, this spec documents exactly what changed.
 *
 * @spec docs/eph/RFC-L2-client-resilience.md (D49 residual, revisited at M44)
 */
import { describe, expect, it } from 'bun:test'

import { createClientHarness, messageUpdatedFrame } from '../index'
import type {
  RuntimeFrame,
  RuntimeMailListViewState,
} from '../../../src/runtime/types'

const flagged = (id: string, receivedAt: string) =>
  messageUpdatedFrame(id, {
    id,
    sourceId: 's',
    receivedAt,
    keywords: ['$flagged'],
    mailboxIds: ['inbox'],
    isRead: false,
    isFlagged: true,
    subject: id,
  })

function flaggedRows(
  frames: RuntimeFrame<RuntimeMailListViewState>[],
): string[] {
  const flaggedIds = new Set<string>()
  for (const f of frames) {
    if (f.type !== 'viewReplace' && f.type !== 'viewSnapshot') continue
    for (const row of f.snapshot.data.rows) {
      const p = row.projection as { id: string; keywords?: string[] }
      if (p.keywords?.includes('$flagged')) flaggedIds.add(p.id)
    }
  }
  return [...flaggedIds]
}

describe('scenario D49: a gap frame resets the incremental view', () => {
  it('drops buffered incremental updates and cancels the scheduled flush', async () => {
    const h = await createClientHarness() // captured coalescer: burst stays buffered
    await h.openView()

    // An incremental update lands and is buffered awaiting the coalesced flush.
    h.emitFrame(flagged('m1', '2026-04-29T10:00:00Z'))
    expect(h.clock.hasScheduledFlush()).toBe(true)

    // The gap: the near node's incremental view is broken.
    h.gapFrame()

    // The scheduled flush was cancelled and the buffered frame dropped: a
    // subsequent flush projects NOTHING (the delta in flight was discarded).
    expect(h.clock.hasScheduledFlush()).toBe(false)
    await h.flush()
    expect(flaggedRows(h.frames)).toEqual([])
    h.dispose()
  })

  it('surfaces the reset upward exactly once (the re-seed trigger)', async () => {
    const h = await createClientHarness()
    await h.openView()
    h.emitFrame(flagged('m1', '2026-04-29T10:00:00Z'))

    h.gapFrame()

    // The renderer is told to re-seed; the runtime then re-serves whole
    // snapshots over the fresh subscription (not modeled here — the baseline is
    // that the client SURFACES the reset rather than silently continuing).
    expect(h.signals.resets).toBe(1)
    h.dispose()
  })

  it('after the reset, a re-served snapshot re-seeds projection (recovery path)', async () => {
    const h = await createClientHarness()
    await h.openView()
    h.emitFrame(flagged('m1', '2026-04-29T10:00:00Z'))
    h.gapFrame()

    // The runtime re-serves the view (whole snapshot). Re-opening re-seeds the
    // store; a fresh authoritative update now projects normally again.
    await h.openView()
    h.emitFrame(flagged('m1', '2026-04-29T10:00:00Z'))
    await h.flush()

    expect(flaggedRows(h.frames)).toContain('m1')
    h.dispose()
  })
})
