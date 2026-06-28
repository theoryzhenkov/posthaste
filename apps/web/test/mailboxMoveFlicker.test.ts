import { describe, expect, it } from 'bun:test'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

import type { EntityStoreHandle } from '../src/runtime/replica/handle'

// Regression for the move/archive/delete flicker (distinct from the keyword
// flicker): a row leaves the inbox, then a STALE view re-serve that still lists
// it must NOT re-add it ("comes back after a blink and stays until refresh").
// The version guard keeps the message base correct; set_view_rows now reconciles
// the served rows against that base instead of clobbering with the served list.
const wasmDir = join(import.meta.dir, '..', 'src', 'runtime', 'wasm')
const present =
  existsSync(join(wasmDir, 'posthaste_link_wasm.js')) &&
  existsSync(join(wasmDir, 'posthaste_link_wasm_bg.wasm'))

function projection(mailboxIds: string[], version: number) {
  return {
    id: 'm1',
    sourceId: 's',
    receivedAt: '2026-04-28T12:00:00Z',
    keywords: [],
    mailboxIds,
    isRead: false,
    isFlagged: false,
    subject: 'm1',
    version,
  }
}

const row = [
  {
    rowKey: 's:m1',
    messageId: 'm1',
    sortKey: { receivedAt: '2026-04-28T12:00:00Z', messageId: 'm1' },
  },
]

describe.skipIf(!present)('mailbox-move flicker (real WASM)', () => {
  it('a stale re-serve does not re-add a moved-out row', async () => {
    const mod = (await import(
      join(wasmDir, 'posthaste_link_wasm.js')
    )) as unknown as {
      initSync(input: { module: BufferSource }): unknown
      EntityStoreHandle: new () => EntityStoreHandle
    }
    mod.initSync({
      module: readFileSync(join(wasmDir, 'posthaste_link_wasm_bg.wasm')),
    })
    const h = new mod.EntityStoreHandle()
    h.registerViewJson(
      'inbox',
      JSON.stringify({
        predicate: { inMailboxes: ['inbox'] },
        sortField: 'date',
        sortDirection: 'desc',
        watermark: null,
      }),
    )
    const inboxIds = () =>
      (JSON.parse(h.projectViewJson('inbox')) ?? []).map(
        (r: { messageId: string }) => r.messageId,
      )

    // m1 in inbox @ v1.
    h.ingestBatchJson(
      JSON.stringify([
        {
          message: {
            messageId: 'm1',
            projection: projection(['inbox'], 1),
            deleted: false,
            countDeltas: [],
          },
        },
      ]),
    )
    h.setViewRowsJson('inbox', JSON.stringify(row), 'null')
    expect(inboxIds()).toEqual(['m1'])

    // The move: m1 → archive @ v2 (authoritative, no client optimism). Leaves inbox.
    h.ingestBatchJson(
      JSON.stringify([
        {
          message: {
            messageId: 'm1',
            projection: projection(['archive'], 2),
            deleted: false,
            countDeltas: [],
          },
        },
      ]),
    )
    expect(inboxIds()).toEqual([])

    // A STALE re-serve still lists m1 in inbox @ v1: the version guard rejects the
    // base (1 < 2) and set_view_rows reconciles it away — m1 must NOT come back.
    h.ingestBatchJson(
      JSON.stringify([
        {
          message: {
            messageId: 'm1',
            projection: projection(['inbox'], 1),
            deleted: false,
            countDeltas: [],
          },
        },
      ]),
    )
    h.setViewRowsJson('inbox', JSON.stringify(row), 'null')
    expect(inboxIds()).toEqual([])
  })
})
