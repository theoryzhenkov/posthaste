import type {
  DecayedCounter,
  PaletteRow,
  RankingContext,
  SearchCandidate,
} from '@/command-search/types'
import { validateSearchQuery } from '@/queryLanguage'
import { normalizeAppliedSearchQuery } from '@/searchQuery'

export const COMMAND_PANEL_STORAGE_KEY = 'posthaste.commandPalette.panelOffset'
export const NO_COMMAND_PALETTE_SELECTION = '__posthaste_no_selection__'

export function commandPaletteEntryValue(candidate: SearchCandidate): string {
  return `candidate:${candidate.id}`
}

function emptyCounter() {
  return { halfLifeMs: 7 * 24 * 60 * 60 * 1000, entries: {} }
}

export function isItemRow(
  row: PaletteRow,
): row is Extract<PaletteRow, { kind: 'item' }> {
  return row.kind === 'item'
}

/** What pressing Enter in the palette should do, given the current state. */
export type PaletteEnterAction = 'apply' | 'run' | 'navigate' | 'none'

/**
 * Resolve the Enter key:
 * - Shift+Enter applies the typed query as the app-wide mail filter.
 * - Enter on a highlighted item runs it.
 * - Enter with nothing highlighted navigates into the in-pane results (selects
 *   the first result) rather than applying an app-wide filter.
 * - Enter with no results is a no-op.
 */
export function resolvePaletteEnter(input: {
  shiftKey: boolean
  hasHighlightedItem: boolean
  hasItems: boolean
}): PaletteEnterAction {
  if (input.shiftKey) return 'apply'
  if (input.hasHighlightedItem) return 'run'
  if (input.hasItems) return 'navigate'
  return 'none'
}

export function currentSearchableServerQuery(query: string): string {
  const validation = validateSearchQuery(query)
  if (validation.state !== 'valid') return ''
  const normalized = normalizeAppliedSearchQuery(query)
  if (!normalized) return ''
  if (normalized.includes(':')) return normalized
  return normalized.length >= 2 ? normalized : ''
}

export function createRankingContext(input: {
  hasSelectedMessage: boolean
  /** Persisted per-command recency/frequency (PLAN-L2 §4.5). Defaults to an
   *  empty counter so callers/tests that don't wire persistence are unaffected. */
  recentCommands?: DecayedCounter
}): RankingContext {
  return {
    now: Date.now(),
    app: {
      route: input.hasSelectedMessage ? 'thread' : 'mailbox',
      hasSelectedMessage: input.hasSelectedMessage,
    },
    session: {
      paletteOpenReason: 'keyboard',
    },
    user: {
      recentCommands: input.recentCommands ?? emptyCounter(),
      recentEntities: emptyCounter(),
      frequentCommands: input.recentCommands ?? emptyCounter(),
      frequentMailboxes: emptyCounter(),
      frequentContacts: emptyCounter(),
      pinnedCommands: [],
      pinnedMailboxes: [],
    },
  }
}
