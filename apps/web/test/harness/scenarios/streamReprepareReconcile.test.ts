/**
 * Scenario — M44 / D112: the reconcile-on-recovery-edge pass.
 *
 * The near-end engine re-prepares a FRESH link (new id) every ~5min (the 300s
 * idle reap) or after sleep. Before M44 that left the client stranded on the
 * DEAD link: server-served views were never re-opened (RC1), their gap frames —
 * including the terminal sync-Ready `account.status_changed` — were lost (RC2),
 * and `linkClient` stayed pinned to the dead link id so even manual recovery
 * 404'd (RC3); only a full reload recovered. This drives the whole client stack
 * (real entity-store adapter + real wasm store + the real `runtimeLinkClient`)
 * over the fake transport and proves BOTH reported field bugs are fixed WITHOUT
 * a reload:
 *
 *  (a) an open server-served view re-serves a fresh base + keeps updating;
 *  (b) `linkClient`'s link id is the new one (open/extend no longer 404 — RC3);
 *  (c) an `accountStatus` snapshot that flipped to Ready during the gap is
 *      re-served on the edge (the empty-mailbox "stuck on Syncing" bug — RC2);
 *  (d) a same-link reconnect does NOT trigger a needless reconcile.
 *
 * @spec docs/eph/RFC-L2-client-resilience.md (M44, D112)
 */
import { afterEach, describe, expect, it } from 'bun:test'

import { createClientHarness, messageUpdatedFrame } from '../index'
import { setRuntimeAdapterForTesting } from '../../../src/runtime/adapter'
import {
  resetRuntimeLinkClientForTesting,
  runtimeLinkClient,
} from '../../../src/runtime/linkClient'
import { getConnectionHealth } from '../../../src/live-store/store'
import type {
  RuntimeFrame,
  RuntimeMailListViewState,
  RuntimeMessagePageRequest,
} from '../../../src/runtime/types'

const VIEW_REQUEST: RuntimeMessagePageRequest = {
  scope: { kind: 'source-mailbox', sourceId: 's', mailboxId: 'inbox' },
  limit: 50,
  sort: 'date',
  sortDir: 'desc',
  operation: { name: 'test' } as never,
}

function lastReplaceIds(
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

const flaggedUpdate = (id: string, receivedAt: string) =>
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

let restoreAdapter: (() => void) | undefined

afterEach(() => {
  restoreAdapter?.()
  restoreAdapter = undefined
  resetRuntimeLinkClientForTesting()
})

describe('scenario M44/D112: reconcile on the link re-prepare edge', () => {
  it('adopts the fresh link id and re-serves open views (RC1/RC3), no reload', async () => {
    const h = await createClientHarness()
    restoreAdapter = setRuntimeAdapterForTesting(h.adapter)
    resetRuntimeLinkClientForTesting()

    const collected: RuntimeFrame<RuntimeMailListViewState>[] = []
    runtimeLinkClient.subscribe({ onFrame: (f) => collected.push(f) })
    await runtimeLinkClient.openMessageListView(VIEW_REQUEST)

    // A view owner (the hook's role) re-opens on the recovery edge.
    let reopenCount = 0
    const off = runtimeLinkClient.onLinkReestablished(() => {
      reopenCount += 1
      void runtimeLinkClient.openMessageListView(VIEW_REQUEST)
    })

    // The engine re-prepared a FRESH link.
    h.transport.reestablishLink('link-B')
    await Promise.resolve()

    // RC1: the open view was re-driven exactly once.
    expect(reopenCount).toBe(1)
    // RC3: the re-open targeted the NEW link id — the first open used the dead
    // 'sess' link, the re-open uses 'link-B' (open/extend/close no longer 404).
    expect(h.transport.viewOpenLinkIds[0]).toBe('sess')
    expect(h.transport.viewOpenLinkIds.at(-1)).toBe('link-B')

    // ...and the view keeps updating on the fresh link WITHOUT a reload.
    await h.flush()
    h.transport.emitFrame(flaggedUpdate('m1', '2026-04-29T10:00:00Z'))
    await h.flush()
    expect(lastReplaceIds(collected)).toContain('m1')

    off()
    h.dispose()
  })

  it('clears a sync-Ready accountStatus lost in the gap by re-serving it (RC2)', async () => {
    const h = await createClientHarness()
    restoreAdapter = setRuntimeAdapterForTesting(h.adapter)
    resetRuntimeLinkClientForTesting()

    // Open the accountStatus object view while the account is still Syncing.
    h.transport.setObjectViewData({ status: 'Syncing' })
    runtimeLinkClient.subscribe({ onFrame: () => {} })
    const opened = await runtimeLinkClient.openView<{ status: string }>({
      family: 'accountStatus',
      payload: {},
      sourceId: null,
    })
    expect(opened.snapshot.data.status).toBe('Syncing')

    // The sync-Ready status_changed rides the lost-frame gap: the server now
    // holds Ready, but the client never saw the clearing frame.
    h.transport.setObjectViewData({ status: 'Ready' })
    let reserved: { status: string } | undefined
    runtimeLinkClient.onLinkReestablished(() => {
      void runtimeLinkClient
        .openView<{ status: string }>({
          family: 'accountStatus',
          payload: {},
          sourceId: null,
        })
        .then((result) => {
          reserved = result.snapshot.data
        })
    })

    h.transport.reestablishLink('link-B')
    await new Promise((resolve) => setTimeout(resolve, 0))

    // The recovery edge re-served the current (Ready) snapshot — the empty-
    // mailbox "stuck on Syncing until reload" bug is cleared without a reload.
    expect(reserved?.status).toBe('Ready')
    h.dispose()
  })

  it('does NOT reconcile on a same-link reconnect (RC discrimination)', async () => {
    const h = await createClientHarness()
    restoreAdapter = setRuntimeAdapterForTesting(h.adapter)
    resetRuntimeLinkClientForTesting()

    runtimeLinkClient.subscribe({ onFrame: () => {} })
    await runtimeLinkClient.openMessageListView(VIEW_REQUEST)

    let reopenCount = 0
    runtimeLinkClient.onLinkReestablished(() => {
      reopenCount += 1
    })

    // A statusless blip: transient sever + reconnect — the SAME link resumes,
    // the engine emits no recovery edge, so no view is needlessly re-served.
    h.severLink()
    h.transport.reconnect()
    await Promise.resolve()

    expect(reopenCount).toBe(0)
    h.dispose()
  })

  it('drives the connection-health FSM through recovering→healthy on the edge', async () => {
    const h = await createClientHarness()
    restoreAdapter = setRuntimeAdapterForTesting(h.adapter)
    resetRuntimeLinkClientForTesting()

    runtimeLinkClient.subscribe({ onFrame: () => {} })
    await runtimeLinkClient.openMessageListView(VIEW_REQUEST)

    expect(getConnectionHealth()).toBe('healthy')

    h.transport.reestablishLink('link-B')
    // Synchronously after the edge the FSM is 'recovering' (the visible blip)...
    expect(getConnectionHealth()).toBe('recovering')
    // ...and returns to 'healthy' once the re-opens are dispatched.
    await Promise.resolve()
    expect(getConnectionHealth()).toBe('healthy')

    h.dispose()
  })
})
