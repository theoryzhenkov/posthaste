import { describe, expect, it } from 'bun:test'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

/**
 * End-to-end smoke test for the WASM boundary (W3): synchronously instantiate
 * the generated module against the built `.wasm` bytes and drive the full
 * `EntityStoreHandle` surface — register a view, ingest an authoritative
 * batch, fold an optimistic mutation, project, then settle. This validates the
 * cargo→wasm-bindgen→wasm-opt pipeline produces a loadable module whose JSON
 * contract matches the host's.
 *
 * The artifacts are generated (`just build-replica-wasm`, gitignored), so the
 * suite skips when they are absent — the `replica-wasm` CI job builds first and
 * runs this file, while the general `frontend` job has no artifact and skips.
 */
const wasmDir = join(import.meta.dir, '..', 'src', 'runtime', 'wasm')
const loaderPath = join(wasmDir, 'posthaste_link_wasm.js')
const binaryPath = join(wasmDir, 'posthaste_link_wasm_bg.wasm')

const artifactsPresent = existsSync(loaderPath) && existsSync(binaryPath)

describe.skipIf(!artifactsPresent)('entity-store WASM boundary smoke', () => {
  it('registers a view, ingests a batch, folds optimism, and projects', async () => {
    const { initSync, EntityStoreHandle } = (await import(loaderPath)) as {
      initSync: (module: { module: BufferSource }) => unknown
      EntityStoreHandle: new () => {
        registerViewJson(viewId: string, argsJson: string): void
        setViewRowsJson(
          viewId: string,
          rowsJson: string,
          watermarkJson: string,
        ): void
        ingestBatchJson(batchJson: string): void
        acceptMutationJson(acceptJson: string): void
        hasPending(): boolean
        settle(mutationId: string, outcome: string): boolean
        projectViewJson(viewId: string): string
        drainDirtyJson(): string
      }
    }

    initSync({ module: readFileSync(binaryPath) })

    const handle = new EntityStoreHandle()
    handle.registerViewJson(
      'inbox',
      JSON.stringify({
        predicate: { inMailbox: 'inbox' },
        sortField: 'date',
        sortDirection: 'desc',
        watermark: null,
      }),
    )
    handle.ingestBatchJson(
      JSON.stringify([
        {
          message: {
            messageId: 'm1',
            projection: {
              id: 'm1',
              sourceId: 'primary',
              receivedAt: '2026-04-29T10:00:00Z',
              mailboxIds: ['inbox'],
              keywords: [],
              isRead: false,
              isFlagged: false,
              subject: 'm1',
            },
            deleted: false,
            countDeltas: [],
          },
        },
      ]),
    )
    handle.drainDirtyJson()
    expect(handle.hasPending()).toBe(false)

    // An optimistic flag folds over the projected message.
    handle.acceptMutationJson(
      JSON.stringify({
        mutationId: 'c1',
        messageId: 'm1',
        assertion: { kind: 'setKeywords', add: ['$flagged'], remove: [] },
      }),
    )
    expect(handle.hasPending()).toBe(true)

    const projected = JSON.parse(handle.projectViewJson('inbox')) as Array<{
      messageId: string
      projection: { isFlagged: boolean; keywords: string[] }
    }>
    expect(projected.map((row) => row.messageId)).toEqual(['m1'])
    expect(projected[0]?.projection.isFlagged).toBe(true)
    expect(projected[0]?.projection.keywords).toContain('$flagged')

    // The dirty drain reports the changed message + view.
    const dirty = JSON.parse(handle.drainDirtyJson()) as Array<
      Record<string, string>
    >
    expect(dirty.some((key) => 'message' in key)).toBe(true)

    // Confirmation retires the pending op without reverting optimism.
    const reverted = handle.settle('c1', 'confirmed')
    expect(reverted).toBe(false)
    expect(handle.hasPending()).toBe(false)
  })
})
