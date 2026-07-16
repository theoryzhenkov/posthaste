/**
 * Typed wrappers for the `posthaste-client-node-wasm` JSON-string boundary.
 *
 * These helpers load and initialize the WASM module once, and translate
 * between runtime contract JSON and the TS discriminated unions the adapter
 * already uses.
 *
 * @spec docs/replication/client-link/L2#3-the-wasm-boundary-posthaste-client-node-wasm
 */
import type { MessageChangeDiff, ReplicaAssertion } from './handle'

export interface ParsedMailOperation {
  messageId: string
  assertion: ReplicaAssertion
}

/** The wasm `NearEndHandle` surface the near-end binding drives (see
 * `src/runtime/nearEnd.ts`); values cross as JSON strings. */
export interface NearEndWasmHandle {
  connect(): Promise<unknown>
  disconnect(): Promise<unknown>
  forward(requestJson: string): Promise<string>
  linkId(): string | undefined
  cursor(): number | undefined
  free(): void
}

interface WasmModule {
  default(): Promise<void>
  /**
   * Synchronously initialize with a wasm binary. Used in Bun tests to avoid
   * `file://` fetch restrictions; in the browser `default()` initializes from
   * the bundled URL.
   */
  initSync(input: BufferSource | WebAssembly.Module): unknown
  parseMailOperation(
    requestJson: string,
    roleMapJson: string,
  ): string | undefined
  invertMessageChangeDiff(diffJson: string): string
  NearEndHandle: new (io: unknown, configJson: string) => NearEndWasmHandle
}

let wasmModulePromise: Promise<WasmModule> | undefined

/** Load + initialize the shared link wasm module once (browser URL fetch, or
 * synchronous binary init under Bun). Shared by the replica helpers here and
 * the near-end engine binding (`src/runtime/nearEnd.ts`). */
export function loadLinkWasmModule(): Promise<WasmModule> {
  return loadWasmModule()
}

async function loadWasmModule(): Promise<WasmModule> {
  wasmModulePromise ??= (async () => {
    const module =
      (await import('../wasm/posthaste_client_node_wasm.js')) as unknown as WasmModule
    if (typeof (globalThis as Record<string, unknown>).Bun !== 'undefined') {
      // Bun test environment: avoid happy-dom's fetch by reading the wasm
      // binary directly. Once initSync() runs, default() is a no-op.
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const fs = (await import('node:fs')) as any
      const wasmPath = new URL(
        '../wasm/posthaste_client_node_wasm_bg.wasm',
        import.meta.url,
      ).pathname
      module.initSync({ module: fs.readFileSync(wasmPath) })
    }
    await module.default()
    return module
  })()
  return wasmModulePromise
}

/**
 * Parse a runtime mutation request (its typed `MailOperation`, flat
 * `{name, args}` on the wire) into the message id and the optimistic assertion
 * the local replica can fold — the shared `MailOperation::fold_effect`
 * projection. Returns `null` when the operation is not locally foldable.
 * `roleMap` is the account's role→mailbox-id map (`{ archive: 'mbx-...' }`),
 * built client-side from the mailbox list; it resolves role moves
 * (archive/trash/restoreToInbox/moveToRole) to `replaceMailboxes`. An empty
 * map → role moves get no optimism (graceful when the mailbox list isn't
 * loaded yet).
 */
export async function parseMailOperation(
  request: {
    name: string
    args?: unknown
    clientMutationId: string
  },
  roleMap: Record<string, string> = {},
): Promise<ParsedMailOperation | null> {
  const module = await loadWasmModule()
  const result = module.parseMailOperation(
    JSON.stringify(request),
    JSON.stringify(roleMap),
  )
  if (result == null) {
    return null
  }
  return JSON.parse(result) as ParsedMailOperation
}

/**
 * Return the inverse of a reversible change-diff. Undo applies the inverse;
 * redo applies the original diff.
 */
export async function invertMessageChangeDiff(
  diff: MessageChangeDiff,
): Promise<MessageChangeDiff> {
  const module = await loadWasmModule()
  return JSON.parse(module.invertMessageChangeDiff(JSON.stringify(diff)))
}
