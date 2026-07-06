import { useMemo } from 'react'
import { useQueryClient } from '@tanstack/react-query'

import type { ActionContext, ActionServices } from '@/actions'
import { createCommandProviders } from '@/command-search/providers'
import { loadRecentCommands } from '@/command-search/recentCommands'
import { recentCachedMessages } from '@/command-search/recentMessages'
import { useCommandSearch } from '@/command-search/useCommandSearch'
import { useMailboxNavigationReadModels } from '@/mailboxNavigationReadModels'

import {
  commandPaletteEntryValue,
  createRankingContext,
  isItemRow,
  NO_COMMAND_PALETTE_SELECTION,
} from './model'

export function useCommandPaletteSearch(input: {
  hasSelectedMessage: boolean
  query: string
  /** Stable accessors for the palette's action context + bound services. */
  getActionContext: () => ActionContext
  getActionServices: () => ActionServices
}) {
  const { hasSelectedMessage, query, getActionContext, getActionServices } =
    input
  const queryClient = useQueryClient()
  const readModels = useMailboxNavigationReadModels()
  const recentMessages = useMemo(
    () => recentCachedMessages(queryClient),
    [queryClient],
  )
  const readModelKey = useMemo(
    () =>
      JSON.stringify({
        smartMailboxes: readModels.smartMailboxes.map((item) => item.id),
        sources: readModels.sources.map((source) => ({
          id: source.id,
          mailboxes: source.mailboxes.map((mailbox) => mailbox.id),
        })),
        tags: readModels.tags.map((tag) => tag.name),
      }),
    [readModels.smartMailboxes, readModels.sources, readModels.tags],
  )
  const providers = useMemo(
    () =>
      createCommandProviders({
        readModels,
        recentMessages,
        getActionContext,
        getActionServices,
      }),
    // readModelKey intentionally collapses unstable React Query wrapper arrays
    // into the domain IDs that affect provider candidates. The action getters
    // are stable (ref-backed), so they never re-create the provider list.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [readModelKey, recentMessages, getActionContext, getActionServices],
  )
  const rankingContext = useMemo(
    () =>
      createRankingContext({
        hasSelectedMessage,
        recentCommands: loadRecentCommands(),
      }),
    [hasSelectedMessage],
  )
  const search = useCommandSearch({ query, context: rankingContext, providers })
  const itemRows = useMemo(
    () => search.session.rows.filter(isItemRow),
    [search.session.rows],
  )
  const activeSelectedIndex = search.session.selectedCandidateId
    ? itemRows.findIndex(
        (row) => row.candidate.id === search.session.selectedCandidateId,
      )
    : -1
  const selectedValue =
    activeSelectedIndex === -1
      ? NO_COMMAND_PALETTE_SELECTION
      : commandPaletteEntryValue(itemRows[activeSelectedIndex].candidate)

  return { activeSelectedIndex, itemRows, search, selectedValue }
}
