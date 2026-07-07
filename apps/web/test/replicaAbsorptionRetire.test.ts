import { describe, expect, it } from 'bun:test'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

import type { EntityStoreHandle } from '../src/runtime/replica/handle'

const wasmDir = join(import.meta.dir, '..', 'src', 'runtime', 'wasm')
const present =
  existsSync(join(wasmDir, 'posthaste_client_node_wasm.js')) &&
  existsSync(join(wasmDir, 'posthaste_client_node_wasm_bg.wasm'))

function projection(keywords: string[], version?: number) {
  return {
    id: 'm1',
    sourceId: 's',
    receivedAt: '2026-04-28T12:00:00Z',
    keywords,
    mailboxIds: ['inbox'],
    isRead: keywords.includes('$seen'),
    isFlagged: keywords.includes('$flagged'),
    subject: 'm1',
    ...(version !== undefined ? { version } : {}),
  }
}

// Retire is confirmed-gated: an op retires only once the authority confirms it
// AND the base absorbs its effect.
async function stillPendingAfterConfirmAndAbsorbingBase(
  seed: string[],
  base: string[],
): Promise<boolean> {
  const mod = (await import(
    join(wasmDir, 'posthaste_client_node_wasm.js')
  )) as unknown as {
    initSync(input: { module: BufferSource }): unknown
    EntityStoreHandle: new () => EntityStoreHandle
  }
  mod.initSync({
    module: readFileSync(join(wasmDir, 'posthaste_client_node_wasm_bg.wasm')),
  })
  const h = new mod.EntityStoreHandle()
  h.registerViewJson(
    'v',
    JSON.stringify({
      predicate: { inMailboxes: ['inbox'] },
      sortField: 'date',
      sortDirection: 'desc',
      watermark: null,
    }),
  )
  h.ingestBatchJson(
    JSON.stringify([
      {
        message: {
          messageId: 'm1',
          projection: projection(seed),
          deleted: false,
        },
      },
    ]),
  )
  h.setViewRowsJson(
    'v',
    JSON.stringify([
      {
        rowKey: 's:m1',
        messageId: 'm1',
        sortKey: { receivedAt: '2026-04-28T12:00:00Z', messageId: 'm1' },
      },
    ]),
    'null',
  )
  h.acceptMutationJson(
    JSON.stringify({
      mutationId: 'op-1',
      messageId: 'm1',
      assertion: { kind: 'setKeywords', add: ['$flagged'], remove: [] },
    }),
  )
  h.ingestBatchJson(
    JSON.stringify([
      {
        message: {
          messageId: 'm1',
          projection: projection(base),
          deleted: false,
        },
      },
    ]),
  )
  h.settle('op-1', 'confirmed')
  return h.hasPending()
}

describe.skipIf(!present)('replica absorption-retire (real WASM)', () => {
  it('retires on confirm when the base carries the effect (single keyword)', async () => {
    expect(
      await stillPendingAfterConfirmAndAbsorbingBase([], ['$flagged']),
    ).toBe(false)
  })
  it('realistic projection ($seen) — retires on confirm (absorption set-insensitive)', async () => {
    expect(
      await stillPendingAfterConfirmAndAbsorbingBase(
        ['$seen'],
        ['$seen', '$flagged'],
      ),
    ).toBe(false)
  })
  it('a stale provider re-serve BEFORE confirm does not revert (confirmed-gating, Bug 1a)', async () => {
    const mod = (await import(
      join(wasmDir, 'posthaste_client_node_wasm.js')
    )) as unknown as {
      initSync(input: { module: BufferSource }): unknown
      EntityStoreHandle: new () => EntityStoreHandle
    }
    mod.initSync({
      module: readFileSync(join(wasmDir, 'posthaste_client_node_wasm_bg.wasm')),
    })
    const h = new mod.EntityStoreHandle()
    h.registerViewJson(
      'v',
      JSON.stringify({
        predicate: { inMailboxes: ['inbox'] },
        sortField: 'date',
        sortDirection: 'desc',
        watermark: null,
      }),
    )
    const flag = () =>
      JSON.parse(h.projectViewJson('v'))?.[0]?.projection.isFlagged === true
    h.ingestBatchJson(
      JSON.stringify([
        {
          message: {
            messageId: 'm1',
            projection: projection([]),
            deleted: false,
          },
        },
      ]),
    )
    h.setViewRowsJson(
      'v',
      JSON.stringify([
        {
          rowKey: 's:m1',
          messageId: 'm1',
          sortKey: { receivedAt: '2026-04-28T12:00:00Z', messageId: 'm1' },
        },
      ]),
      'null',
    )
    h.acceptMutationJson(
      JSON.stringify({
        mutationId: 'op-1',
        messageId: 'm1',
        assertion: { kind: 'setKeywords', add: ['$flagged'], remove: [] },
      }),
    )
    // The optimistic echo carries the flag (does NOT retire — unconfirmed)...
    h.ingestBatchJson(
      JSON.stringify([
        {
          message: {
            messageId: 'm1',
            projection: projection(['$flagged']),
            deleted: false,
          },
        },
      ]),
    )
    // ...and a stale sync re-serve (no flag) arrives BEFORE confirm: the op is
    // still folded, so the flag survives (the common during-sync case).
    h.ingestBatchJson(
      JSON.stringify([
        {
          message: {
            messageId: 'm1',
            projection: projection([]),
            deleted: false,
          },
        },
      ]),
    )
    expect(flag()).toBe(true)
    expect(h.hasPending()).toBe(true)
  })
  it('BUG 1b guard target: a stale re-serve (older version) after confirm is rejected', async () => {
    const mod = (await import(
      join(wasmDir, 'posthaste_client_node_wasm.js')
    )) as unknown as {
      initSync(input: { module: BufferSource }): unknown
      EntityStoreHandle: new () => EntityStoreHandle
    }
    mod.initSync({
      module: readFileSync(join(wasmDir, 'posthaste_client_node_wasm_bg.wasm')),
    })
    const h = new mod.EntityStoreHandle()
    h.registerViewJson(
      'v',
      JSON.stringify({
        predicate: { inMailboxes: ['inbox'] },
        sortField: 'date',
        sortDirection: 'desc',
        watermark: null,
      }),
    )
    const flag = () =>
      JSON.parse(h.projectViewJson('v'))?.[0]?.projection.isFlagged === true
    // seed unflagged @ v1
    h.ingestBatchJson(
      JSON.stringify([
        {
          message: {
            messageId: 'm1',
            projection: projection([], 1),
            deleted: false,
          },
        },
      ]),
    )
    h.setViewRowsJson(
      'v',
      JSON.stringify([
        {
          rowKey: 's:m1',
          messageId: 'm1',
          sortKey: { receivedAt: '2026-04-28T12:00:00Z', messageId: 'm1' },
        },
      ]),
      'null',
    )
    h.acceptMutationJson(
      JSON.stringify({
        mutationId: 'op-1',
        messageId: 'm1',
        assertion: { kind: 'setKeywords', add: ['$flagged'], remove: [] },
      }),
    )
    // provider applies the flag @ v2; confirm it (the op retires)
    h.ingestBatchJson(
      JSON.stringify([
        {
          message: {
            messageId: 'm1',
            projection: projection(['$flagged'], 2),
            deleted: false,
          },
        },
      ]),
    )
    h.settle('op-1', 'confirmed')
    // late STALE re-serve @ v1 (1 < 2) must be rejected — the flag holds
    h.ingestBatchJson(
      JSON.stringify([
        {
          message: {
            messageId: 'm1',
            projection: projection([], 1),
            deleted: false,
          },
        },
      ]),
    )
    expect(flag()).toBe(true)
  })
})
