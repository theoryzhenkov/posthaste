/**
 * Commands barrel + registration bootstrap.
 *
 * Importing this module is what POPULATES the registry: the side-effect import
 * of every `defs/*` module runs its top-level `registerActions(...)`. Because it
 * is a bare side-effect import, no bundler elides it, so any consumer that pulls
 * `resolveActions` (or the types) from here is guaranteed the registry is filled
 * before it resolves.
 *
 * Every surface should import the resolver + types from THIS barrel rather than
 * reaching into `resolve.ts` / `defs/*` directly, so registration is never
 * accidentally skipped. Only `app/` (and this subtree) may import it —
 * components consume resolved actions through `lib/command` (R11).
 */

// Side-effect imports: register the definitions (charter split — navigation,
// compose, mail, global). Section sorting is stable, so cross-file order only
// needs to stay deterministic.
import './defs/navigation'
import './defs/compose'
import './defs/mail'
import './defs/global'

export { resolveActions } from './resolve'
export {
  firstMatchingChord,
  formatChord,
  formatChords,
  resolveKeyboardAction,
  runResolvedWithConfirm,
  matchesChord,
  type ActionConfirm,
  type ChordEvent,
} from './keyboard'
export { getAction, allActions } from './registry'
export { CommandDispatcher } from './dispatcher'
export {
  buildDetailHeaderActions,
  buildRowContextMenu,
  messageTargetFromSelection,
} from './bind'
export type {
  ActionContext,
  ActionParamOption,
  ActionServices,
  MessageTarget,
} from './types'
