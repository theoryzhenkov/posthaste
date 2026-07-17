/**
 * Actions barrel + registration bootstrap (PLAN-L2, Slice 2).
 *
 * Importing this module is what POPULATES the registry: the side-effect import
 * of every `defs/*` module runs its top-level `registerActions(...)`. Because it
 * is a bare side-effect import, no bundler elides it, so any consumer that pulls
 * `resolveActions` (or the types) from here is guaranteed the registry is filled
 * before it resolves — this is how the registry stays populated now that the old
 * `contextualActions.ts` shim (which used to trigger registration) is gone.
 *
 * Every surface should import the resolver + types from THIS barrel rather than
 * reaching into `resolve.ts` / `defs/*` directly, so registration is never
 * accidentally skipped.
 *
 * @spec docs/eph/PLAN-L2-action-registry.md
 */

// Side-effect imports: register the action definitions (message + app-level).
import './defs/message'
import './defs/app'

export { formatChord, formatChords } from './formatChord'
export { resolveActions, type ResolvedAction } from './resolve'
export {
  resolveKeyboardAction,
  runResolvedWithConfirm,
  matchesChord,
  shortcutMatches,
  type ActionConfirm,
  type ChordEvent,
} from './keyboard'
export { getAction, allActions } from './registry'
export type {
  ActionContext,
  ActionDefinition,
  ActionParamOption,
  ActionSection,
  ActionServices,
  ActionSurface,
  MessageTarget,
} from './types'
