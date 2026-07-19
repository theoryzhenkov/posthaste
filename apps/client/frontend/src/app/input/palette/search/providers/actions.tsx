import { createElement } from 'react'

import {
  formatChord,
  resolveActions,
  type ActionContext,
  type ActionServices,
} from '@/commands'

import type { CommandPaletteEntry, SearchProvider } from '../types'
import { matchedCandidatePage } from './shared'

/**
 * The single registry-backed palette provider.
 *
 * Resolves the palette surface from the unified registry
 * (`resolveActions(ctx, services)`) and maps each
 * {@link import('@/commands').ResolvedAction} to a `CommandPaletteEntry`. The
 * registry unlocks:
 *
 * - contextual availability — trash view surfaces "Delete permanently", drafts
 *   surface "Discard draft", etc., because the same `isAvailable` predicates
 *   the context menu uses run here too; an action that cannot run right now
 *   (failed availability or enablement) is simply not listed;
 * - shortcut hints — `formatChord(def.shortcut)` rides along on the entry.
 *
 * `ctx`/`services` are read through stable getters (the palette keeps them in
 * refs) so the provider identity stays stable across renders — the search
 * pipeline re-runs on query/ranking-context change, not on every keystroke of
 * app state.
 */
export function createActionProvider(input: {
  getContext: () => ActionContext
  getServices: () => ActionServices
}): SearchProvider {
  const provider: SearchProvider = {
    id: 'commands',
    label: 'Commands',
    vertical: 'command',
    async search(req) {
      const resolved = resolveActions(input.getContext(), input.getServices())
      const entries: CommandPaletteEntry[] = resolved.map((action) => ({
        id: action.def.id,
        kind: 'command',
        label: action.title,
        keywords: action.def.keywords ?? '',
        icon: createElement(action.icon, {
          size: 15,
          strokeWidth: 1.7,
          className: 'text-muted-foreground',
        }),
        // A PARAMETERIZED action (Move to…, Snooze…) doesn't run on select — it
        // pushes the palette into its pick-step (a searchable option list), so
        // the palette must stay open.
        action: action.params
          ? { kind: 'open-action-params', actionId: action.def.id }
          : { kind: 'action', actionId: action.def.id },
        closeOnSelect: action.params ? false : undefined,
        shortcut: formatChord(action.def.shortcut),
      }))

      return matchedCandidatePage(provider, entries, req)
    },
  }
  return provider
}
