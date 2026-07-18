/**
 * The registry's global dispatcher — the R4 sink for input outside the mail
 * shell's KeyboardController.
 *
 * ONE window keydown listener, mounted once at the app root by
 * {@link CommandDispatcher}. Hosts register {@link CommandScope}s (via
 * `lib/command.useCommandScope`) instead of listening themselves: the app root
 * binds `desktop` (devtools), a surface host binds `surfaceHost.close`
 * (Escape), a mounted composer binds `compose.send` (⌘Enter). Scopes are
 * consulted last-registered-first — mirroring the visual stacking of their
 * hosts — and the first scope that resolves an available action for the chord
 * wins, exactly reproducing the independent per-host listeners this replaces.
 *
 * Repeats are skipped (a held chord fires once). While an editable element is
 * focused only `inEditable` chords fire. An action whose resolution carries a
 * confirm/param gate is skipped here — these scopes host no dialog; gated
 * actions belong to surfaces that do (KeyboardController, header, menus).
 */
import { useEffect, useMemo, useRef, type ReactNode } from 'react'
import {
  CommandScopeContext,
  type CommandScope,
  type CommandScopeRegistry,
} from '../lib/command'
import { isEditableKeyboardTarget } from '../lib/dom'
import { resolveKeyboardAction, shortcutMatches } from './keyboard'
import type { ActionContext, ActionServices, ShortcutChord } from './types'

/** The resolver context of a scope: no targets, no view — scope services are
 *  the only capabilities, which is exactly what gates availability. */
function scopeContext(scope: CommandScope): ActionContext {
  return {
    targets: [],
    viewRole: null,
    activePane: 'list',
    surface: 'keyboard',
    inputOwner: scope.owner,
    hasPendingMutation: false,
    connection: 'unknown',
  }
}

function chordAllowsEditable(
  shortcut: ShortcutChord | readonly ShortcutChord[] | undefined,
  event: KeyboardEvent,
): boolean {
  if (!shortcut) return false
  const chords = Array.isArray(shortcut)
    ? (shortcut as readonly ShortcutChord[])
    : [shortcut as ShortcutChord]
  return chords.some(
    (chord) =>
      (chord.inEditable ?? false) && shortcutMatches(chord, event),
  )
}

export function CommandDispatcher({
  scope,
  children,
}: {
  /** The app root's base scope (e.g. the `desktop` devtools binding). */
  scope: CommandScope
  children: ReactNode
}) {
  const scopesRef = useRef<CommandScope[]>([])
  const baseRef = useRef(scope)
  baseRef.current = scope

  const registry = useMemo<CommandScopeRegistry>(
    () => ({
      register: (next) => {
        scopesRef.current = [...scopesRef.current, next]
        return () => {
          scopesRef.current = scopesRef.current.filter((s) => s !== next)
        }
      },
    }),
    [],
  )

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.repeat) return
      const editable = isEditableKeyboardTarget(event.target)
      const stack = [...scopesRef.current].reverse().concat(baseRef.current)
      for (const candidate of stack) {
        const resolved = resolveKeyboardAction(
          event,
          scopeContext(candidate),
          candidate.services as ActionServices,
        )
        if (!resolved) continue
        if (editable && !chordAllowsEditable(resolved.def.shortcut, event)) {
          continue
        }
        // No confirm/param host here — a gated action never fires from a bare
        // scope chord (none of the scope-bound defs carry gates today).
        if (resolved.confirm || resolved.params !== undefined) return
        event.preventDefault()
        void resolved.execute()
        return
      }
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [])

  return (
    <CommandScopeContext.Provider value={registry}>
      {children}
    </CommandScopeContext.Provider>
  )
}
