/**
 * Contextual resolver (PLAN-L2, Slice 1).
 *
 * Given an {@link ActionContext} and the injected {@link ActionServices}, filter
 * the registry by requesting surface → availability → enablement, apply the
 * context-derived title/icon, and order by section (then registration order).
 * Pure given `(ctx, services)`; the shape mirrors the old builder so section
 * separators fall out of adjacent-section changes exactly as the context menu
 * renders them today.
 *
 * @spec docs/eph/PLAN-L2-action-registry.md
 */
import type { LucideIcon } from 'lucide-react'
import { allActions } from './registry'
import type {
  ActionContext,
  ActionDefinition,
  ActionSection,
  ActionServices,
} from './types'

export interface ResolvedAction {
  def: ActionDefinition
  /** Context-applied. */
  title: string
  icon: LucideIcon
  enabled: boolean
  disabledReason?: string
  /** Bound runner: applies confirm gating (later slices), then `def.run`. */
  execute: () => void | Promise<void>
}

/** Menu / palette section order. */
const SECTION_ORDER: readonly ActionSection[] = [
  'open',
  'compose-reply',
  'state',
  'organize',
  'move',
  'navigate',
  'app',
]

function bind(
  def: ActionDefinition,
  ctx: ActionContext,
  services: ActionServices,
): ResolvedAction {
  const title = typeof def.title === 'function' ? def.title(ctx) : def.title
  // Lucide icons are forwardRef exotic *objects* (typeof 'object'), so a runtime
  // `typeof === 'function'` selects only the `(ctx) => LucideIcon` form; the cast
  // just tells TS that (TS otherwise treats the callable component as a function).
  const icon: LucideIcon =
    typeof def.icon === 'function'
      ? (def.icon as (c: ActionContext) => LucideIcon)(ctx)
      : def.icon
  const enablement = def.isEnabled?.(ctx) ?? true
  const enabled = enablement === true
  const disabledReason =
    enablement !== true && typeof enablement === 'object'
      ? enablement.reason
      : undefined
  return {
    def,
    title,
    icon,
    enabled,
    disabledReason,
    // Slice 1 ports carry no `confirm`, so this matches the old direct `run`.
    // The confirm-dialog host is wired in a later slice.
    execute: () => def.run(ctx, services),
  }
}

/**
 * Resolve the ordered action list for `ctx.surface`.
 *
 * `includeDisabled` keeps shown-but-disabled entries (palette discoverability);
 * menus pass it falsy so disabled actions vanish, matching today's context menu.
 */
export function resolveActions(
  ctx: ActionContext,
  services: ActionServices,
  opts?: { includeDisabled?: boolean },
): ResolvedAction[] {
  return allActions()
    .filter((d) => d.surfaces.includes(ctx.surface))
    .filter((d) => d.isAvailable?.(ctx, services) ?? true)
    .map((d) => bind(d, ctx, services))
    .filter((r) => r.enabled || opts?.includeDisabled)
    .sort(
      (a, b) =>
        SECTION_ORDER.indexOf(a.def.section) -
        SECTION_ORDER.indexOf(b.def.section),
    )
}
