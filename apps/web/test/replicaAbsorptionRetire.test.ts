import { describe, expect, it } from 'bun:test'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

const wasmDir = join(import.meta.dir, '..', 'src', 'runtime', 'wasm')
const present =
  existsSync(join(wasmDir, 'posthaste_link_wasm.js')) &&
  existsSync(join(wasmDir, 'posthaste_link_wasm_bg.wasm'))

function projection(keywords: string[]) {
  return {
    id: 'm1',
    sourceId: 's',
    receivedAt: '2026-04-28T12:00:00Z',
    keywords,
    mailboxIds: ['inbox'],
    isRead: keywords.includes('$seen'),
    isFlagged: keywords.includes('$flagged'),
    subject: 'm1',
  }
}

async function stillPendingAfterAbsorbingBase(
  seed: string[],
  base: string[],
): Promise<boolean> {
  const mod = (await import(join(wasmDir, 'posthaste_link_wasm.js'))) as any
  mod.initSync({
    module: readFileSync(join(wasmDir, 'posthaste_link_wasm_bg.wasm')),
  })
  const h = new mod.EntityStoreHandle()
  h.registerViewJson(
    'v',
    JSON.stringify({
      predicate: { inMailbox: 'inbox' },
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
          countDeltas: [],
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
          countDeltas: [],
        },
      },
    ]),
  )
  return h.hasPending()
}

describe.skipIf(!present)('replica absorption-retire (real WASM)', () => {
  it('retires the op when the base carries the effect (single keyword)', async () => {
    expect(await stillPendingAfterAbsorbingBase([], ['$flagged'])).toBe(false)
  })
  it('realistic projection ($seen present) — op retires (absorption is set-insensitive)', async () => {
    expect(
      await stillPendingAfterAbsorbingBase(['$seen'], ['$seen', '$flagged']),
    ).toBe(false)
  })
  it.failing(
    'BUG: a late stale provider re-serve reverts a retired flag (the flicker)',
    async () => {
      const mod = (await import(join(wasmDir, 'posthaste_link_wasm.js'))) as any
      mod.initSync({
        module: readFileSync(join(wasmDir, 'posthaste_link_wasm_bg.wasm')),
      })
      const h = new mod.EntityStoreHandle()
      h.registerViewJson(
        'v',
        JSON.stringify({
          predicate: { inMailbox: 'inbox' },
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
              countDeltas: [],
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
              projection: projection(['$flagged']),
              deleted: false,
              countDeltas: [],
            },
          },
        ]),
      )
      h.settle('op-1', 'confirmed')
      h.ingestBatchJson(
        JSON.stringify([
          {
            message: {
              messageId: 'm1',
              projection: projection([]),
              deleted: false,
              countDeltas: [],
            },
          },
        ]),
      )
      expect(flag()).toBe(true)
    },
  )
})
