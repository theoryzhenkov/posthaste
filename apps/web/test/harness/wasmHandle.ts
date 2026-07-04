/**
 * Load the REAL committed client WASM `EntityStoreHandle` for the harness, once.
 *
 * The deterministic testkit drives the shipped wasm store (not a TS re-impl of
 * the engine), exactly as `entityStoreAdapter.test.ts` does — so the harness
 * proves orchestration against the engine that actually ships. The node wasm
 * bundle is a committed artifact; we `initSync` it from the binary once (avoids
 * the `file://` fetch) and memoize the module so every harness in a run shares
 * the one initialization.
 *
 * @spec docs/eph/RFC-L2-client-resilience.md#D119
 */
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import type { EntityStoreHandle } from '../../src/runtime/replica/handle'

const wasmDir = join(import.meta.dir, '..', '..', 'src', 'runtime', 'wasm')

// eslint-disable-next-line @typescript-eslint/no-explicit-any
let wasmModule: any | null = null

/** Initialize (once) and return a factory that mints fresh real store handles. */
export async function loadRealHandleFactory(): Promise<
  () => EntityStoreHandle
> {
  if (!wasmModule) {
    const modulePath = join(wasmDir, 'posthaste_client_node_wasm.js')
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const mod = (await import(modulePath)) as any
    mod.initSync({
      module: readFileSync(join(wasmDir, 'posthaste_client_node_wasm_bg.wasm')),
    })
    wasmModule = mod
  }
  return () => new wasmModule.EntityStoreHandle() as EntityStoreHandle
}
