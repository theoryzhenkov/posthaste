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
 * The single registry-backed palette provider (PLAN-L2, Slice 3).
 *
 * Replaces the two hand-rolled providers (`commands.tsx` + `tagActions.tsx`):
 * it resolves the palette surface from the unified registry
 * (`resolveActions(ctx, services, { includeDisabled: true })`) and maps each
 * {@link import('@/actions').ResolvedAction} to a `CommandPaletteEntry`. The
 * enrichments the registry unlocks fall out for free:
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
 *
 * Keeps the provider id `commands` + vertical `command` so the ranker's
 * "Commands" section, per-provider limit, and vertical prior are unchanged.
 *
 * @spec docs/eph/PLAN-L2-action-registry.md
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
        action: { kind: 'action', actionId: action.def.id },
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
