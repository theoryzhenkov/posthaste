import type {
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
      recentCommands: emptyCounter(),
      recentEntities: emptyCounter(),
      frequentCommands: emptyCounter(),
      frequentMailboxes: emptyCounter(),
      frequentContacts: emptyCounter(),
      pinnedCommands: [],
      pinnedMailboxes: [],
    },
  }
}
