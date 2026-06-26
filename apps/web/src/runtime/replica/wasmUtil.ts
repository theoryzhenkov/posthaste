/**
 * Typed wrappers for the `posthaste-link-wasm` JSON-string boundary.
 *
 * These helpers load and initialize the WASM module once, and translate
 * between runtime contract JSON and the TS discriminated unions the adapter
 * already uses.
 *
 * @spec docs/replication/client-link/L2#3-the-wasm-boundary-posthaste-link-wasm
 */
import type { MessageChangeDiff, ReplicaAssertion } from './handle'

export interface ParsedMessageMutation {
  messageId: string
  assertion: ReplicaAssertion
}

interface WasmModule {
  default(): Promise<void>
  parseMessageMutation(requestJson: string): string | undefined
  invertMessageChangeDiff(diffJson: string): string
}

let wasmModulePromise: Promise<WasmModule> | undefined

async function loadWasmModule(): Promise<WasmModule> {
  wasmModulePromise ??= (async () => {
    const module =
      (await import('../wasm/posthaste_link_wasm.js')) as unknown as WasmModule
    await module.default()
    return module
  })()
  return wasmModulePromise
}

/**
 * Parse a runtime mutation request into the message id and the optimistic
 * assertion the local replica can fold. Returns `null` when the mutation is
 * not locally foldable (e.g. role moves that need account resolution).
 */
export async function parseMessageMutation(request: {
  name: string
  args?: unknown
  clientMutationId: string
}): Promise<ParsedMessageMutation | null> {
  const module = await loadWasmModule()
  const result = module.parseMessageMutation(JSON.stringify(request))
  if (result == null) {
    return null
  }
  return JSON.parse(result) as ParsedMessageMutation
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
