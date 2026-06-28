import { describe, expect, it } from 'bun:test'

import type { MessageSummary } from '../src/api/types'
import { setupDomEnvironment } from './dom-env'
import {
  RenderLog,
  RenderProbe,
  messageUpdatedFrame,
  mutationNotificationFrame,
  type RenderedRow,
} from './renderProbe'

setupDomEnvironment()

// --- detector unit tests (pure; no WASM, no React) ---

function r(id: string, over: Partial<RenderedRow> = {}): RenderedRow {
  return {
    messageId: id,
    isRead: false,
    isFlagged: false,
    mailboxIds: ['inbox'],
    keywords: [],
    ...over,
  }
}

/** Build a log from a sequence of row-sets (one per snapshot). */
function log(rowsPerSnap: RenderedRow[][]): RenderLog {
  return new RenderLog(
    rowsPerSnap.map((rows, i) => ({ after: String(i), rows })),
  )
}

describe('RenderLog detector', () => {
  it('detects a disappear-reappear flash (red)', () => {
    const l = log([[r('m1'), r('m2')], [r('m1')], [r('m1'), r('m2')]])
    expect(() => l.assertNoFlicker('m2')).toThrow(/presence flicker/)
  })

  it('detects a keyword revert (red)', () => {
    const l = log([
      [r('m1')],
      [r('m1', { keywords: ['$flagged'], isFlagged: true })],
      [r('m1')],
    ])
    expect(() => l.assertNoFlicker('m1')).toThrow(/keyword flicker/)
  })

  it('detects a mailbox move revert (red)', () => {
    const l = log([
      [r('m1')],
      [r('m1', { mailboxIds: ['archive'] })],
      [r('m1')],
    ])
    expect(() => l.assertNoFlicker('m1')).toThrow(/move flicker/)
  })

  it('detects a read revert (red)', () => {
    const l = log([[r('m1')], [r('m1', { isRead: true })], [r('m1')]])
    expect(() => l.assertNoFlicker('m1')).toThrow(/read flicker/)
  })

  it('detects a whole-view snapshot regression when no id is given (red)', () => {
    const l = log([[r('m1'), r('m2')], [r('m1')], [r('m1'), r('m2')]])
    expect(() => l.assertNoFlicker()).toThrow(/presence flicker/)
  })

  it('passes a clean monotonic delete (green)', () => {
    const l = log([[r('m1'), r('m2')], [r('m1')]])
    expect(() => l.assertNoFlicker()).not.toThrow()
  })

  it('passes a clean monotonic flag set (green)', () => {
    const l = log([
      [r('m1')],
      [r('m1', { keywords: ['$flagged'], isFlagged: true })],
    ])
    expect(() => l.assertNoFlicker('m1')).not.toThrow()
  })
})

// --- wiring: real adapter + real WASM + real hook + real render ---

function msg(id: string, over: Partial<MessageSummary> = {}): MessageSummary {
  return {
    id,
    sourceId: 's',
    sourceName: 'S',
    sourceThreadId: 't1',
    conversationId: 'c1',
    subject: id,
    fromName: 'Sender',
    fromEmail: 'sender@example.org',
    to: [],
    preview: 'Preview',
    receivedAt: '2026-04-28T12:00:00Z',
    hasAttachment: false,
    isRead: false,
    isFlagged: false,
    mailboxIds: ['inbox'],
    keywords: [],
    ...over,
  }
}

function setFlagged(
  id: string,
  clientMutationId: string,
): RuntimeRunMutationRequestLike {
  return {
    sessionId: 'sess',
    name: 'message.setKeywords',
    args: {
      sourceId: 's',
      messageId: id,
      command: { add: ['$flagged'], remove: [] },
    },
    clientMutationId,
  }
}

// Local shape to avoid importing the full runtime union for the test body.
type RuntimeRunMutationRequestLike = {
  sessionId: string
  name: string
  args: Record<string, unknown>
  clientMutationId: string
}

describe('RenderProbe (real adapter + real WASM)', () => {
  it('opens through the real adapter and records the served rows', async () => {
    const probe = await RenderProbe.open({
      rows: [msg('m1'), msg('m2')],
    })
    try {
      expect(probe.renderedRows().map((r) => r.messageId)).toEqual(['m1', 'm2'])
      expect(() => probe.intoLog().assertNoFlicker()).not.toThrow()
    } finally {
      probe.unmount()
    }
  })

  it('a flag toggle holds through confirm + base catch-up (Bug 1a, green)', async () => {
    const probe = await RenderProbe.open({ rows: [msg('m1')] })
    try {
      // Optimistic flag (op folded before the base carries it).
      await probe.runMutation(setFlagged('m1', 'c1') as never)
      probe.record('after-accept')
      // Confirmation outruns the authoritative base — must NOT revert.
      await probe.emitFrame(mutationNotificationFrame('c1', 'confirmed'))
      probe.record('after-confirm')
      // The base catches up (flagged); op retires by absorption. Still flagged.
      await probe.emitFrame(
        messageUpdatedFrame(
          'm1',
          msg('m1', { isFlagged: true, keywords: ['$flagged'] }),
        ),
      )
      probe.record('after-base')

      const log = probe.intoLog()
      expect(() => log.assertNoFlicker('m1')).not.toThrow()
      // From the optimistic flag onward, the row stays flagged — no revert.
      expect(log.snapshots.slice(1).every((s) => s.rows[0]?.isFlagged)).toBe(
        true,
      )
    } finally {
      probe.unmount()
    }
  })
})
