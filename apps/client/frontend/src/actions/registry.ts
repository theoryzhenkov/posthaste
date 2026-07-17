/**
 * Unified action registry.
 *
 * A module-level collection populated once, at import time, from the static
 * definition files (`actions/defs/*`). Insertion order is preserved and used by
 * the resolver as the within-section tiebreak, so registration order is
 * meaningful.
 *
 * No React here — pure data in, pure data out — so it is unit-testable exactly
 * like `components/keyboard/dispatch.ts`.
 */
import type { ActionDefinition } from './types'

const registry = new Map<string, ActionDefinition>()

/** Register a batch of definitions. Throws on a duplicate id — a duplicate is a
 *  definition bug, not a runtime condition. */
export function registerActions(defs: readonly ActionDefinition[]): void {
  for (const def of defs) {
    if (registry.has(def.id)) {
      throw new Error(`duplicate action ${def.id}`)
    }
    registry.set(def.id, def)
  }
}

export function getAction(id: string): ActionDefinition | undefined {
  return registry.get(id)
}

/** All registered definitions in registration order. */
export function allActions(): readonly ActionDefinition[] {
  return [...registry.values()]
}
