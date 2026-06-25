import { describe, expect, it } from 'bun:test'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

/**
 * End-to-end smoke test for the WASM boundary (W3): synchronously instantiate
 * the generated module against the built `.wasm` bytes and drive the full
 * `MailListReplicaHandle` surface — ingest a served base, fold an optimistic
 * mutation, project, then settle. This validates the cargo→wasm-bindgen→wasm-opt
 * pipeline produces a loadable module whose JSON contract matches the host's.
 *
 * The artifacts are generated (`just build-replica-wasm`, gitignored), so the
 * suite skips when they are absent — the `replica-wasm` CI job builds first and
 * runs this file, while the general `frontend` job has no artifact and skips.
 */
const wasmDir = join(import.meta.dir, '..', 'src', 'runtime', 'wasm')
const loaderPath = join(wasmDir, 'posthaste_link_wasm.js')
const binaryPath = join(wasmDir, 'posthaste_link_wasm_bg.wasm')

const artifactsPresent = existsSync(loaderPath) && existsSync(binaryPath)

describe.skipIf(!artifactsPresent)('replica WASM boundary smoke', () => {
  it('instantiates and folds an optimistic mutation over a served base', async () => {
    const { initSync, MailListReplicaHandle } = (await import(loaderPath)) as {
      initSync: (module: { module: BufferSource }) => unknown
      MailListReplicaHandle: new () => {
        ingestJson(rows: string): void
        acceptJson(accept: string): void
        hasPending(): boolean
        settle(mutationId: string, outcome: string): boolean
        projectJson(mailboxId?: string | null): string
      }
    }

    initSync({ module: readFileSync(binaryPath) })

    const handle = new MailListReplicaHandle()
    handle.ingestJson(
      JSON.stringify([
        { messageId: 'm1', projection: { id: 'm1', keywords: [] } },
        { messageId: 'm2', projection: { id: 'm2', keywords: [] } },
      ]),
    )

    expect(handle.hasPending()).toBe(false)

    handle.acceptJson(
      JSON.stringify({
        mutationId: 'c1',
        messageId: 'm1',
        assertion: { kind: 'setKeywords', add: ['$seen'], remove: [] },
      }),
    )
    expect(handle.hasPending()).toBe(true)

    const optimistic = JSON.parse(handle.projectJson()) as Array<{
      id: string
      keywords: string[]
    }>
    expect(optimistic.map((row) => row.id)).toEqual(['m1', 'm2'])
    const folded = optimistic.find((row) => row.id === 'm1')
    expect(folded?.keywords).toContain('$seen')

    // Confirmation retires the pending op without reverting optimism.
    const reverted = handle.settle('c1', 'confirmed')
    expect(reverted).toBe(false)
    expect(handle.hasPending()).toBe(false)
  })

  it('folds an applyDiff assertion over keywords and mailboxes', async () => {
    const { initSync, MailListReplicaHandle } = (await import(loaderPath)) as {
      initSync: (module: { module: BufferSource }) => unknown
      MailListReplicaHandle: new () => {
        ingestJson(rows: string): void
        acceptJson(accept: string): void
        hasPending(): boolean
        settle(mutationId: string, outcome: string): boolean
        projectJson(mailboxId?: string | null): string
      }
    }

    initSync({ module: readFileSync(binaryPath) })

    const handle = new MailListReplicaHandle()
    handle.ingestJson(
      JSON.stringify([
        {
          messageId: 'm1',
          projection: {
            id: 'm1',
            keywords: ['$seen'],
            mailboxIds: ['inbox'],
          },
        },
      ]),
    )

    handle.acceptJson(
      JSON.stringify({
        mutationId: 'c2',
        messageId: 'm1',
        assertion: {
          kind: 'applyDiff',
          diff: {
            keywords: { added: ['$flagged'], removed: ['$seen'] },
            mailboxes: { added: [], removed: ['inbox'] },
          },
        },
      }),
    )

    const optimistic = JSON.parse(handle.projectJson()) as Array<{
      id: string
      keywords: string[]
      mailboxIds: string[]
    }>
    const folded = optimistic.find((row) => row.id === 'm1')
    expect(folded?.keywords).toContain('$flagged')
    expect(folded?.keywords).not.toContain('$seen')
    expect(folded?.mailboxIds).not.toContain('inbox')

    const inboxView = JSON.parse(handle.projectJson('inbox')) as Array<{
      id: string
    }>
    expect(inboxView.some((row) => row.id === 'm1')).toBe(false)
  })
})
