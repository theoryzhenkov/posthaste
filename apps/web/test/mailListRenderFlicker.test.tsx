import { describe, expect, it } from 'bun:test'

import type { MessageSummary } from '../src/api/types'
import { setupDomEnvironment } from './dom-env'
import {
  RenderProbe,
  messageUpdatedFrame,
  mutationNotificationFrame,
} from './renderProbe'

setupDomEnvironment()

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

type MoveRequest = {
  sessionId: string
  name: 'message.replaceMailboxes'
  args: { sourceId: string; messageId: string; mailboxIds: string[] }
  clientMutationId: string
}

function move(
  id: string,
  mailboxIds: string[],
  clientMutationId: string,
): MoveRequest {
  return {
    sessionId: 'sess',
    name: 'message.replaceMailboxes',
    args: { sourceId: 's', messageId: id, mailboxIds },
    clientMutationId,
  }
}

/**
 * Issue 2 repro: a mailbox-move/delete briefly flashes the view as it was at
 * last load (the moved/deleted rows reappear). Drives the REAL adapter + real
 * WASM + real hook + real render through the probe and records the trajectory.
 */
describe('mailList render flicker (issue 2)', () => {
  it('a plain move removes the row with no flash (no stale re-serve)', async () => {
    const probe = await RenderProbe.open({
      rows: [msg('m1'), msg('m2'), msg('m3')],
    })
    try {
      await probe.runMutation(move('m1', ['archive'], 'c1'))
      probe.record('after-move-m1')
      await probe.runMutation(move('m2', ['archive'], 'c2'))
      probe.record('after-move-m2')

      const log = probe.intoLog()
      // No mutations/frames re-served the moved rows -> no reappear.
      expect(() => log.assertNoFlicker()).not.toThrow()
      expect(log.snapshots.at(-1)!.rows.map((r) => r.messageId)).toEqual(['m3'])
    } finally {
      probe.unmount()
    }
  })

  it('a stale re-serve with no version re-inserts the moved row after retire (Bug 1b unguarded, red)', async () => {
    const probe = await RenderProbe.open({ rows: [msg('m1'), msg('m2')] })
    try {
      await probe.runMutation(move('m1', ['archive'], 'c1'))
      probe.record('after-move')
      // Confirm outruns the base — op stays folded (absorption-gated).
      await probe.emitFrame(mutationNotificationFrame('c1', 'confirmed'))
      probe.record('after-confirm')
      // Base catches up to archive -> op retires (absorbed). Row stays gone.
      await probe.emitFrame(
        messageUpdatedFrame('m1', msg('m1', { mailboxIds: ['archive'] })),
      )
      probe.record('after-base-archive')
      // Stale re-serve: m1 back in inbox, NO version -> unguarded clobber of a
      // retired op's base -> the row reappears (the flash).
      await probe.emitFrame(
        messageUpdatedFrame('m1', msg('m1', { mailboxIds: ['inbox'] })),
      )
      probe.record('after-stale-serve')
      // Correcting sync: m1 archived again -> row leaves.
      await probe.emitFrame(
        messageUpdatedFrame('m1', msg('m1', { mailboxIds: ['archive'] })),
      )
      probe.record('after-correct')

      const log = probe.intoLog()
      // m1 presence in inbox: present -> gone -> gone -> gone -> present -> gone.
      // That reappear is the flash; the detector must catch it.
      expect(() => log.assertNoFlicker('m1')).toThrow(/presence flicker/)
    } finally {
      probe.unmount()
    }
  })

  it('a stale re-serve with an older version is rejected (fix a, green)', async () => {
    // m1 opens at version 2 (the authority watermark); the move does not bump
    // it, so a stale re-serve at version 1 is strictly older.
    const probe = await RenderProbe.open({
      rows: [msg('m1', { version: 2 }), msg('m2')],
    })
    try {
      await probe.runMutation(move('m1', ['archive'], 'c1'))
      probe.record('after-move')
      await probe.emitFrame(mutationNotificationFrame('c1', 'confirmed'))
      probe.record('after-confirm')
      await probe.emitFrame(
        messageUpdatedFrame(
          'm1',
          msg('m1', { mailboxIds: ['archive'], version: 2 }),
        ),
      )
      probe.record('after-base-archive')
      // Stale re-serve at version 1 (older) -> the version guard rejects it.
      await probe.emitFrame(
        messageUpdatedFrame(
          'm1',
          msg('m1', { mailboxIds: ['inbox'], version: 1 }),
        ),
      )
      probe.record('after-stale-serve')

      const log = probe.intoLog()
      expect(() => log.assertNoFlicker('m1')).not.toThrow()
      // m1 stays archived (never reappears).
      expect(log.snapshots.at(-1)!.rows.map((r) => r.messageId)).toEqual(['m2'])
    } finally {
      probe.unmount()
    }
  })
})
