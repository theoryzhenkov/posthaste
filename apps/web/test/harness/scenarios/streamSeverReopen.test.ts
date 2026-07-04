/**
 * Scenario (a) — F1 / M40: the stream is severed and the reopen 404s.
 *
 * The RFC's highest-value failure: an idle-reaped or daemon-restarted link
 * returns 404 on the subscribe GET; the wasm near-end classifies that as
 * PERMANENT and `run()` returns, halting all live updates until reload. The
 * classification itself is Rust-side; this pins what the CLIENT LAYER SEES at
 * the TS seam — the shim surfaces a permanent error (not a transient one), the
 * stream stops flowing, and a reconnect resumes projection. It is the
 * regression bed for M40's landed fix (re-prepare on stale-link 4xx) and the
 * M44 reconcile pass.
 *
 * @spec docs/eph/RFC-L2-client-resilience.md (F1, M40)
 */
import { describe, expect, it } from 'bun:test'

import { createClientHarness, messageUpdatedFrame } from '../index'
import type {
  RuntimeFrame,
  RuntimeMailListViewState,
} from '../../../src/runtime/types'

function idsOf(
  frames: RuntimeFrame<RuntimeMailListViewState>[],
): string[] | undefined {
  const last = [...frames]
    .reverse()
    .find((f) => f.type === 'viewReplace' || f.type === 'viewSnapshot')
  if (last?.type !== 'viewReplace' && last?.type !== 'viewSnapshot') {
    return undefined
  }
  return last.snapshot.data.rows.map((r) => (r.projection as { id: string }).id)
}

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

describe('scenario F1/M40: sever with 404-on-reopen', () => {
  it('surfaces a 4xx reopen as a PERMANENT error at the TS seam (not transient)', async () => {
    const h = await createClientHarness()
    await h.openView()

    h.severWith(404)

    // The client layer sees exactly one permanent error — the shape M40's
    // re-prepare keys off — and NOT a transient (which would just backoff).
    expect(h.signals.permanentErrors).toHaveLength(1)
    expect(h.signals.transientErrors).toHaveLength(0)
    expect(String(h.signals.permanentErrors[0])).toContain('404')
    h.dispose()
  })

  it('halts live updates while severed: no frame can flow until reconnect', async () => {
    const h = await createClientHarness()
    await h.openView()
    h.severWith(404)

    // A would-be live update can't be delivered over a dead stream — the halt
    // the RFC describes (all live updates stop until the link is re-prepared).
    expect(() => h.emitFrame(flagged('m1', '2026-04-29T10:00:00Z'))).toThrow(
      /severed/,
    )
    h.dispose()
  })

  it('resumes projection after a reconnect (the M40 re-prepare shape)', async () => {
    const h = await createClientHarness()
    await h.openView()
    h.severWith(404)

    // M40: the link re-prepares and the stream resumes WITHOUT a reload. Model
    // the resumed stream + assert the client re-projects the replayed update.
    h.reconnect()
    h.emitFrame(flagged('m1', '2026-04-29T10:00:00Z'))
    await h.flush()

    const rows = idsOf(h.frames)
    expect(rows).toContain('m1')
    const last = [...h.frames].reverse().find((f) => f.type === 'viewReplace')
    const m1 =
      last?.type === 'viewReplace'
        ? last.snapshot.data.rows.find(
            (r) => (r.projection as { id: string }).id === 'm1',
          )
        : undefined
    expect((m1?.projection as { keywords?: string[] })?.keywords).toContain(
      '$flagged',
    )
    h.dispose()
  })

  it('a status-less sever surfaces as TRANSIENT (recoverable), not permanent', async () => {
    const h = await createClientHarness()
    await h.openView()

    h.severLink()

    expect(h.signals.transientErrors).toHaveLength(1)
    expect(h.signals.permanentErrors).toHaveLength(0)
    h.dispose()
  })
})
