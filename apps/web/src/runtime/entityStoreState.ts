/**
 * The entity-store activation flag (2e): a runtime intent facade.
 *
 * Non-runtime modules (e.g. `domain-cache`'s invalidation gate) need to ask
 * whether the entity store owns the mail list, but the boundary check forbids
 * importing the adapter selection module (`runtime/adapter`) directly. This
 * facade holds the flag + readers so consumers depend on the narrow query, not
 * the adapter wiring.
 *
 * @spec docs/eph/PLAN-L2-client-link-reactive-store (2e.3)
 */
let entityStoreActive = false

/** Whether the client-layer WASM entity-store adapter is active (the store owns
 * the mail-list rows + mailbox counts). */
export function isEntityStoreAdapterActive(): boolean {
  return entityStoreActive
}

/** Mark the entity-store adapter active (called by `installEntityStoreAdapter`
 * once the WASM store wraps the base adapter). */
export function markEntityStoreActive(): void {
  entityStoreActive = true
}

/** Test-only: set the flag to exercise the 2e.3 invalidation gate. */
export function setEntityStoreActiveForTesting(active: boolean): () => void {
  const previous = entityStoreActive
  entityStoreActive = active
  return () => {
    entityStoreActive = previous
  }
}
