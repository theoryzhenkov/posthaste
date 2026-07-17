import { useCallback, useMemo } from 'react'
import { useQueryClient } from '@tanstack/react-query'

import type { ActionContext, ActionServices } from '@/actions'
import { createActionParamProvider } from '@/command-search/providers/actionParams'
import { createCommandProviders } from '@/command-search/providers'
import type { MessageSearchRequest } from '@/command-search/providers/messages'
import { loadRecentCommands } from '@/command-search/recentCommands'
import { recentCachedMessages } from '@/command-search/recentMessages'
import { useCommandSearch } from '@/command-search/useCommandSearch'
import { fetchQuery, useMailClient } from '@/data'
import type { MailListResult } from '@/gen'
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
  /** When set, the palette is in a parameterized action's PICK-STEP: the whole
   *  provider set is swapped for that action's option list, so search/selection
   *  machinery is reused unchanged for the second step. */
  paramStep: { actionId: string; label: string } | null
}) {
  const {
    hasSelectedMessage,
    query,
    getActionContext,
    getActionServices,
    paramStep,
  } = input
  const queryClient = useQueryClient()
  const mailClient = useMailClient()
  const readModels = useMailboxNavigationReadModels()
  const recentMessages = useMemo(
    () => recentCachedMessages(queryClient),
    [queryClient],
  )
  // The message provider's free-text window: one `mailList` evaluation per
  // search request — the palette renders the answer, it never filters mail.
  const searchMessages = useCallback(
    (req: MessageSearchRequest) =>
      fetchQuery<MailListResult>(mailClient, {
        mailList: {
          freeText: req.freeText,
          cursor: req.cursor,
          limit: req.limit,
        },
      }),
    [mailClient],
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
      paramStep
        ? [
            createActionParamProvider({
              actionId: paramStep.actionId,
              label: paramStep.label,
              getContext: getActionContext,
              getServices: getActionServices,
            }),
          ]
        : createCommandProviders({
            readModels,
            recentMessages,
            searchMessages,
            getActionContext,
            getActionServices,
          }),
    // readModelKey intentionally collapses unstable React Query wrapper arrays
    // into the domain IDs that affect provider candidates. The action getters
    // are stable (ref-backed), so they never re-create the provider list.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [
      readModelKey,
      recentMessages,
      searchMessages,
      getActionContext,
      getActionServices,
      paramStep,
    ],
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
