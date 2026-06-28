// De-risk probe for undo-history Phase 1 option (a): the client captures an
// invertible change-diff LOCALLY from its folded base + the mutation's
// assertion, mirroring the runtime's `read_fold_state` + `capture_diff`
// (`from_before_after(prev, curr)`). If this is clean, the client can own the
// undo history without the runtime sending a per-mutation diff — undo/redo
// become local-optimistic with no round trip.
//
// Drives the REAL WASM EntityStoreHandle (not a fake).
import { describe, expect, it } from 'bun:test'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import type {
  EntityStoreHandle,
  EntityStoreHandleFactory,
  MessageChangeDiff,
  ReplicaAssertion,
} from '../src/runtime/replica/handle'
import { invertMessageChangeDiff } from '../src/runtime/replica/wasmUtil'

const WASM_DIR = join(import.meta.dir, '..', 'src', 'runtime', 'wasm')

let factory: EntityStoreHandleFactory

async function loadFactory(): Promise<EntityStoreHandleFactory> {
  const mod = (await import(
    join(WASM_DIR, 'posthaste_link_wasm.js')
  )) as unknown as {
    initSync(input: { module: BufferSource }): unknown
    EntityStoreHandle: new () => EntityStoreHandle
  }
  mod.initSync({
    module: readFileSync(join(WASM_DIR, 'posthaste_link_wasm_bg.wasm')),
  })
  return () => new mod.EntityStoreHandle()
}

async function setup(): Promise<void> {
  factory ??= await loadFactory()
}

function ingest(
  handle: EntityStoreHandle,
  messageId: string,
  mailboxIds: string[],
  keywords: string[] = [],
): void {
  handle.ingestBatchJson(
    JSON.stringify([
      {
        message: {
          messageId,
          projection: {
            id: messageId,
            sourceId: 'acc1',
            mailboxIds,
            keywords,
            receivedAt: '2026-06-28T00:00:00Z',
          },
          deleted: false,
          countDeltas: [],
        },
      },
    ]),
  )
}

const MOVE_TO_TRASH: ReplicaAssertion = {
  kind: 'replaceMailboxes',
  mailboxIds: ['trash'],
}

describe('client-local undo diff capture (captureMutationDiffJson)', () => {
  it('captures a mailbox move diff from the folded base WITHOUT applying it', async () => {
    await setup()
    const handle = factory()
    ingest(handle, 'm1', ['drafts'])

    const diff = JSON.parse(
      handle.captureMutationDiffJson('m1', JSON.stringify(MOVE_TO_TRASH)),
    ) as MessageChangeDiff

    expect(diff).toEqual({
      keywords: { added: [], removed: [] },
      mailboxes: { added: ['trash'], removed: ['drafts'] },
    })
    // capture is a pure read — the message did not move
    const untouched = JSON.parse(handle.messageJson('m1'))
    expect(untouched.mailboxIds).toEqual(['drafts'])
  })

  it('captures a keyword toggle diff', async () => {
    await setup()
    const handle = factory()
    ingest(handle, 'm2', ['inbox'], ['$seen'])

    const toggle: ReplicaAssertion = {
      kind: 'setKeywords',
      add: ['$flagged'],
      remove: ['$seen'],
    }
    const diff = JSON.parse(
      handle.captureMutationDiffJson('m2', JSON.stringify(toggle)),
    ) as MessageChangeDiff

    expect(diff).toEqual({
      keywords: { added: ['$flagged'], removed: ['$seen'] },
      mailboxes: { added: [], removed: [] },
    })
  })

  it('inverse(diff) restores the pre-mutation state (the undo vehicle)', async () => {
    await setup()
    const handle = factory()
    ingest(handle, 'm3', ['drafts'])

    const diff = JSON.parse(
      handle.captureMutationDiffJson('m3', JSON.stringify(MOVE_TO_TRASH)),
    ) as MessageChangeDiff

    const inverse = await invertMessageChangeDiff(diff)
    // inverse swaps added <-> removed — applying it undoes the move
    expect(inverse.mailboxes).toEqual({ added: ['drafts'], removed: ['trash'] })
    expect(inverse.keywords).toEqual({ added: [], removed: [] })
  })

  it('reads the CURRENT base after a base update (no stale diff — the convergence guard)', async () => {
    await setup()
    const handle = factory()
    ingest(handle, 'm4', ['drafts'])
    // a sync delivers a base update: the message is now in drafts + extra
    ingest(handle, 'm4', ['drafts', 'extra'])

    const diff = JSON.parse(
      handle.captureMutationDiffJson('m4', JSON.stringify(MOVE_TO_TRASH)),
    ) as MessageChangeDiff

    // the diff removes BOTH current mailboxes (drafts + extra), proving the
    // capture read the up-to-date base — not a stale snapshot captured before
    // the sync. This is the property that makes client-owned history safe
    // alongside a churning sync (the flicker/QA scenario).
    expect(diff.mailboxes.removed).toEqual(['drafts', 'extra'])
    expect(diff.mailboxes.added).toEqual(['trash'])
  })

  it('returns null for a message not yet held (deferred; no diff to record)', async () => {
    await setup()
    const handle = factory()
    const diffJson = handle.captureMutationDiffJson(
      'unknown',
      JSON.stringify(MOVE_TO_TRASH),
    )
    expect(diffJson).toBe('null')
  })
})
