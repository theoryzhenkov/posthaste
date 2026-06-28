import { describe, expect, it } from 'bun:test'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

// Fix (b): archive (a role move) now resolves to a replaceMailboxes op via the
// account's role→mailbox-id map, so fix (a)'s equal-version hold can keep it
// folded through the unconfirmed window. This is the user's actual archive
// flicker: archive → row leaves → a stale same-modseq re-serve must NOT bring
// it back. Models real provider behavior (a local move does not bump modseq, so
// the moved base and a stale re-serve share v5; the op retires only at v6).

const wasmDir = join(import.meta.dir, '..', 'src', 'runtime', 'wasm')
const present =
  existsSync(join(wasmDir, 'posthaste_link_wasm.js')) &&
  existsSync(join(wasmDir, 'posthaste_link_wasm_bg.wasm'))

function projection(mailboxIds: string[], version: number) {
  return {
    id: 'm1',
    sourceId: 'acct',
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
  rowKey: 'acct:m1',
  messageId: 'm1',
  sortKey: { receivedAt: '2026-04-28T12:00:00Z', messageId: 'm1' },
}

describe.skipIf(!present)('archive optimism (real WASM)', () => {
  it('archive resolves to replaceMailboxes via the role map', async () => {
    const mod = (await import(join(wasmDir, 'posthaste_link_wasm.js'))) as any
    mod.initSync({
      module: readFileSync(join(wasmDir, 'posthaste_link_wasm_bg.wasm')),
    })
    const request = JSON.stringify({
      name: 'message.moveToRole',
      args: { sourceId: 'acct', messageId: 'm1', role: 'archive' },
      clientMutationId: 'arc-1',
    })
    // No role map → no optimism (mailbox list not loaded yet; graceful).
    expect(mod.parseMessageMutation(request, '{}')).toBeUndefined()
    // With the account's archive mailbox → replaceMailboxes([mbx-archive]).
    const out = JSON.parse(
      mod.parseMessageMutation(request, '{"archive":"mbx-archive"}'),
    )
    expect(out.messageId).toBe('m1')
    expect(out.assertion.kind).toBe('replaceMailboxes')
    expect(out.assertion.mailbox_ids).toEqual(['mbx-archive'])
  })

  it('an archive op holds through the equal-version stale window, retiring at the bump', async () => {
    const mod = (await import(join(wasmDir, 'posthaste_link_wasm.js'))) as any
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
        (r: any) => r.messageId === 'm1',
      )

    // m1 in inbox @ v5.
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

    // Archive: resolve the role → replaceMailboxes op, accept it (row leaves).
    const parsed = JSON.parse(
      mod.parseMessageMutation(
        JSON.stringify({
          name: 'message.moveToRole',
          args: { sourceId: 'acct', messageId: 'm1', role: 'archive' },
          clientMutationId: 'arc-1',
        }),
        '{"archive":"mbx-archive"}',
      ),
    )
    h.acceptMutationJson(
      JSON.stringify({
        mutationId: 'arc-1',
        messageId: 'm1',
        assertion: parsed.assertion,
      }),
    )
    expect(inInbox()).toBe(false)

    // Provider's same-modseq [mbx-archive]@5 (move applied, modseq not bumped) +
    // the verdict confirms: the op must NOT retire at 5 == 5.
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
    expect(h.settle('arc-1', 'confirmed')).toBe(false) // not reverted — holds

    // Stale [inbox]@5 re-serve (equal version) clobbers the base — but the op is
    // still pending, folding [mbx-archive] over it, so m1 stays out of inbox.
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
    expect(inInbox()).toBe(false)

    // Provider confirms with modseq+1 ([mbx-archive]@6): strictly higher → retire.
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
