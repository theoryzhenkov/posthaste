import { describe, expect, it } from 'bun:test'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

import type { EntityStoreHandle } from '../src/runtime/replica/handle'

// Independent end-to-end verification of BOTH archive fixes against the real
// WASM: fix (b) resolves message.moveToRole{role:archive} -> replaceMailboxes via the role map
// (parseMailOperation), and fix (a) holds the resulting op through an
// equal-version stale re-serve so the archived row leaves and STAYS.

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
const m1Row = {
  rowKey: 's:m1',
  messageId: 'm1',
  sortKey: { receivedAt: '2026-04-28T12:00:00Z', messageId: 'm1' },
}

describe.skipIf(!present)('archive flicker end-to-end (real WASM)', () => {
  it('archive resolves to replaceMailboxes via the role map; empty map = no optimism', async () => {
    const mod = (await import(
      join(wasmDir, 'posthaste_link_wasm.js')
    )) as unknown as {
      initSync(input: { module: BufferSource }): unknown
      EntityStoreHandle: new () => EntityStoreHandle
      parseMailOperation(
        requestJson: string,
        roleMapJson: string,
      ): string | undefined
    }
    mod.initSync({
      module: readFileSync(join(wasmDir, 'posthaste_link_wasm_bg.wasm')),
    })
    const request = JSON.stringify({
      name: 'message.moveToRole',
      args: { sourceId: 's', messageId: 'm1', role: 'archive' },
      clientMutationId: 'arc-1',
    })

    // With the account's role map → resolves to a foldable ReplaceMailboxes.
    const resolved = mod.parseMailOperation(
      request,
      JSON.stringify({ archive: 'mbx-archive' }),
    )
    expect(resolved).not.toBeNull()
    const parsed = JSON.parse(resolved)
    expect(parsed.messageId).toBe('m1')
    expect(parsed.assertion.kind).toBe('replaceMailboxes')
    expect(parsed.assertion.mailboxIds).toEqual(['mbx-archive'])

    // No role map (mailbox list not cached yet) → None = falls back to pass-through.
    // wasm-bindgen maps Rust None to `undefined`; the adapter's `if (!result)`
    // treats it as pass-through (no optimism).
    expect(mod.parseMailOperation(request, '{}')).toBeUndefined()
  })

  it('an archived row leaves immediately and survives an equal-version stale re-serve', async () => {
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
    const inInbox = () =>
      (JSON.parse(h.projectViewJson('inbox')) ?? []).some(
        (r: { messageId: string }) => r.messageId === 'm1',
      )

    h.ingestBatchJson(
      JSON.stringify([
        {
          message: {
            messageId: 'm1',
            projection: projection(['inbox'], 5),
            deleted: false,
            countDeltas: [],
          },
        },
      ]),
    )
    h.setViewRowsJson('inbox', JSON.stringify([m1Row]), 'null')
    expect(inInbox()).toBe(true)

    // The adapter resolves + accepts the archive op (fix b), then runs it.
    const parsed = JSON.parse(
      mod.parseMailOperation(
        JSON.stringify({
          name: 'message.moveToRole',
          args: { sourceId: 's', messageId: 'm1', role: 'archive' },
          clientMutationId: 'arc-1',
        }),
        JSON.stringify({ archive: 'mbx-archive' }),
      ),
    )
    h.acceptMutationJson(
      JSON.stringify({
        mutationId: 'arc-1',
        messageId: parsed.messageId,
        assertion: parsed.assertion,
      }),
    )
    expect(inInbox()).toBe(false) // leaves immediately

    // Move applied locally (same modseq) + confirm; then the equal-version stale
    // inbox re-serve. The op holds [mbx-archive] (fix a) → stays gone.
    h.ingestBatchJson(
      JSON.stringify([
        {
          message: {
            messageId: 'm1',
            projection: projection(['mbx-archive'], 5),
            deleted: false,
            countDeltas: [],
          },
        },
      ]),
    )
    h.settle('arc-1', 'confirmed')
    h.ingestBatchJson(
      JSON.stringify([
        {
          message: {
            messageId: 'm1',
            projection: projection(['inbox'], 5),
            deleted: false,
            countDeltas: [],
          },
        },
      ]),
    )
    h.setViewRowsJson('inbox', JSON.stringify([m1Row]), 'null')
    expect(inInbox()).toBe(false) // does NOT come back

    // Provider confirms the move at the bumped modseq → op retires; still gone.
    h.ingestBatchJson(
      JSON.stringify([
        {
          message: {
            messageId: 'm1',
            projection: projection(['mbx-archive'], 6),
            deleted: false,
            countDeltas: [],
          },
        },
      ]),
    )
    expect(inInbox()).toBe(false)
  })
})
