/**
 * Unified action registry (PLAN-L2, Slice 1).
 *
 * A module-level collection populated once, at import time, from the static
 * definition files (`actions/defs/*`). Insertion order is preserved and used by
 * the resolver as the within-section tiebreak, so registration order is
 * meaningful (mirrors the push order of the old `contextualActions.ts` builder).
 *
 * No React here — pure data in, pure data out — so it is unit-testable exactly
 * like `components/keyboard/dispatch.ts`.
 *
 * @spec docs/eph/PLAN-L2-action-registry.md
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
