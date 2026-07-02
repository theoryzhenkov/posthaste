import { describe, expect, it } from 'bun:test'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

import type { EntityStoreHandle } from '../src/runtime/replica/handle'

// The .20 set_view_rows-reconcile fix assumed a move bumps the per-message
// version (the regression test used [archive]@v2 > [inbox]@v1, so the guard
// rejected the stale re-serve). But a LOCAL move does NOT advance modseq — the
// provider hasn't confirmed it — so the [archive] base and a stale [inbox]
// re-serve carry the SAME version. The guard is strict-< (equal = accept), so
// the stale re-serve clobbers the membership and the row comes back + stays.
// This models the real provider behavior the unit fix missed.

const wasmDir = join(import.meta.dir, '..', 'src', 'runtime', 'wasm')
const present =
  existsSync(join(wasmDir, 'posthaste_client_node_wasm.js')) &&
  existsSync(join(wasmDir, 'posthaste_client_node_wasm_bg.wasm'))

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

describe.skipIf(!present)(
  'mailbox-move flicker — equal version (real WASM)',
  () => {
    it.failing(
      'a same-version stale re-serve must not re-add a moved-out row',
      async () => {
        const mod = (await import(
          join(wasmDir, 'posthaste_client_node_wasm.js')
        )) as unknown as {
          initSync(input: { module: BufferSource }): unknown
          EntityStoreHandle: new () => EntityStoreHandle
        }
        mod.initSync({
          module: readFileSync(
            join(wasmDir, 'posthaste_client_node_wasm_bg.wasm'),
          ),
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

        // Local move to archive — version NOT bumped (provider hasn't confirmed): [archive]@v5.
        h.ingestBatchJson(
          JSON.stringify([
            {
              message: {
                messageId: 'm1',
                projection: projection(['archive'], 5),
                deleted: false,
                countDeltas: [],
              },
            },
          ]),
        )
        expect(inInbox()).toBe(false) // the blink: it leaves

        // Stale re-serve still lists m1 in inbox at the SAME version (5 == 5):
        // strict-< guard accepts it (not 5 < 5), set_view_rows then re-holds it.
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

        expect(inInbox()).toBe(false) // must stay gone — but it comes back
      },
    )

    // With fix (a) (version-gated retire) on main, the op holds through the
    // equal-version echo + confirm + stale re-serve and the row stays gone.
    it('moveToMailbox optimism holds the move through an equal-version stale re-serve', async () => {
      // The optimism path (moveToMailbox carries ReplaceMailboxes). The op SHOULD
      // hold [archive] until the provider confirms with a HIGHER version, but
      // confirmed-gated retire retires it on the equal-version local echo, so a
      // subsequent equal-version stale re-serve still clobbers. Models the real
      // values: local move leaves modseq unchanged → move-base@v5 == stale@v5.
      const mod = (await import(
        join(wasmDir, 'posthaste_client_node_wasm.js')
      )) as unknown as {
        initSync(input: { module: BufferSource }): unknown
        EntityStoreHandle: new () => EntityStoreHandle
      }
      mod.initSync({
        module: readFileSync(
          join(wasmDir, 'posthaste_client_node_wasm_bg.wasm'),
        ),
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
      // Optimistic move to archive (the adapter accepts it before the mutation runs).
      // The wire field is `mailboxIds` (replica-core rename; `mailbox_ids` was the
      // pre-rename shape — enum rename_all renames tags, not struct fields).
      h.acceptMutationJson(
        JSON.stringify({
          mutationId: 'mv-1',
          messageId: 'm1',
          assertion: { kind: 'replaceMailboxes', mailboxIds: ['archive'] },
        }),
      )
      expect(inInbox()).toBe(false)
      // The move's own echo carries [archive] at the UNCHANGED version v5 (local
      // move doesn't bump modseq); confirm settles it → op retires by absorption.
      h.ingestBatchJson(
        JSON.stringify([
          {
            message: {
              messageId: 'm1',
              projection: projection(['archive'], 5),
              deleted: false,
              countDeltas: [],
            },
          },
        ]),
      )
      h.settle('mv-1', 'confirmed')
      // Equal-version stale re-serve → accepted → m1 back (op already retired).
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
      expect(inInbox()).toBe(false) // op holds [archive] -> stays gone

      // The provider finally confirms the move at the bumped modseq; the op now
      // retires (strictly-higher version), and the row stays gone.
      h.ingestBatchJson(
        JSON.stringify([
          {
            message: {
              messageId: 'm1',
              projection: projection(['archive'], 6),
              deleted: false,
              countDeltas: [],
            },
          },
        ]),
      )
      expect(inInbox()).toBe(false)
    })
  },
)
