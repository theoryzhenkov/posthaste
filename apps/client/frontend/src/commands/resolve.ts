/**
 * Contextual resolver.
 *
 * Given an {@link ActionContext} and the injected {@link ActionServices}, filter
 * the registry by requesting surface → availability → enablement, apply the
 * context-derived title/icon, and order by section (then registration order).
 * Pure given `(ctx, services)`.
 */
import type { LucideIcon } from 'lucide-react'
import type { ResolvedActionView } from '../lib/command'
import { allActions } from './registry'
import type {
  ActionContext,
  ActionDefinition,
  ActionParamOption,
  ActionSection,
  ActionServices,
} from './types'

/** A resolved action: the flattened, domain-free view every surface renders
 *  (`lib/command.ResolvedActionView` — components receive exactly that) plus
 *  the full definition for registry-side consumers (palette search keywords,
 *  shortcut hints, the keyboard matcher). */
export interface ResolvedAction extends ResolvedActionView {
  def: ActionDefinition
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
  const enabled = def.isEnabled?.(ctx) ?? true
  const params = def.resolveParams?.(ctx, services)
  const confirm =
    typeof def.confirm === 'function' ? def.confirm(ctx) : def.confirm
  return {
    def,
    id: def.id,
    section: def.section,
    destructive: def.destructive ?? false,
    title,
    icon,
    enabled,
    confirm,
    params,
    execute: () => def.run(ctx, services),
    executeWith: params
      ? (param: ActionParamOption) => def.run(ctx, services, param)
      : undefined,
  }
}

/**
 * Resolve the ordered action list for `ctx.surface`.
 *
 * `includeDisabled` keeps disabled entries in the resolution — the keyboard
 * dispatcher uses it so a chord the table CLAIMS but cannot run right now still
 * swallows the event. Every rendering surface (menu, palette, header) passes it
 * falsy, so disabled actions are simply not shown.
 */
export function resolveActions(
  ctx: ActionContext,
  services: ActionServices,
  opts?: { includeDisabled?: boolean },
): ResolvedAction[] {
  return (
    allActions()
      .filter((d) => d.surfaces.includes(ctx.surface))
      .filter((d) => d.isAvailable?.(ctx, services) ?? true)
      .map((d) => bind(d, ctx, services))
      .filter((r) => r.enabled || opts?.includeDisabled)
      // A parameterized action with NOTHING to pick (e.g. move-to-mailbox when
      // every candidate mailbox is excluded) is dropped like a failed
      // availability check. A DISABLED resolution is kept (its options are
      // naturally empty without a target) so `includeDisabled` — the keyboard
      // dispatcher's chord-claiming path — still sees it.
      .filter(
        (r) => r.params === undefined || r.params.length > 0 || !r.enabled,
      )
      .sort(
        (a, b) =>
          SECTION_ORDER.indexOf(a.def.section) -
          SECTION_ORDER.indexOf(b.def.section),
      )
  )
}
