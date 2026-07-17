import { createElement } from 'react'

import {
  resolveActions,
  type ActionContext,
  type ActionServices,
} from '@/actions'

import { matchesQuery } from '../match'
import type { CommandPaletteEntry, SearchProvider } from '../types'
import { candidateFromEntry } from './shared'

/**
 * The palette PICK-STEP provider for a parameterized action.
 *
 * When the user selects a parameterized command ("Move to…", "Snooze…") — or a
 * keyboard chord opens the palette straight into a picker — the palette swaps
 * its provider set for this single provider: it re-resolves the action for the
 * CURRENT palette context (same availability/enablement as the root list) and
 * emits one candidate per `ResolvedAction.params` option. The palette's normal
 * list/search/selection machinery does the rest, so typing filters options
 * exactly like commands.
 *
 * Options that no longer resolve (message deselected, action unavailable in
 * this view) yield an empty page — the palette shows its normal empty state.
 */
export function createActionParamProvider(input: {
  actionId: string
  /** Section label above the options (the action's title, e.g. "Move to…"). */
  label: string
  getContext: () => ActionContext
  getServices: () => ActionServices
}): SearchProvider {
  const provider: SearchProvider = {
    id: 'action-params',
    label: input.label,
    vertical: 'command',
    async search(req) {
      const resolved = resolveActions(input.getContext(), input.getServices())
        .filter((action) => action.def.id === input.actionId)
        .at(0)
      const entries: CommandPaletteEntry[] = (resolved?.params ?? []).map(
        (option) => ({
          id: `${input.actionId}:${option.id}`,
          kind: 'command',
          label: option.label,
          keywords: option.keywords ?? '',
          icon: createElement(option.icon ?? resolved!.icon, {
            size: 15,
            strokeWidth: 1.7,
            className: 'text-muted-foreground',
          }),
          action: {
            kind: 'run-action-param',
            actionId: input.actionId,
            param: option,
          },
        }),
      )

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
