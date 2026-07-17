import { createElement } from 'react'

import {
  formatChord,
  resolveActions,
  type ActionContext,
  type ActionServices,
} from '@/actions'

import { matchesQuery } from '../match'
import type { CommandPaletteEntry, SearchProvider } from '../types'
import { candidateFromEntry } from './shared'

/**
 * The single registry-backed palette provider.
 *
 * Resolves the palette surface from the unified registry
 * (`resolveActions(ctx, services, { includeDisabled: true })`) and maps each
 * {@link import('@/actions').ResolvedAction} to a `CommandPaletteEntry`. The
 * registry unlocks:
 *
 * - contextual availability — trash view surfaces "Delete permanently", drafts
 *   surface "Discard draft", etc., because the same `isAvailable` predicates
 *   the context menu uses run here too;
 * - disabled-with-reason — selection-scoped actions render greyed with their
 *   `disabledReason` instead of vanishing (`includeDisabled: true`);
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
      const resolved = resolveActions(input.getContext(), input.getServices(), {
        includeDisabled: true,
      })
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
        disabled: !action.enabled,
        disabledReason: action.disabledReason,
        shortcut: formatChord(action.def.shortcut),
      }))

      const matched = entries.filter((entry) =>
        matchesQuery(req.query, entry.label, entry.keywords),
      )

      return {
        candidates: matched
          .slice(0, req.limit)
          .map((entry, index) =>
            candidateFromEntry(provider, entry, req.query, index),
          ),
        nextCursor: null,
      }
    },
  }
  return provider
}
