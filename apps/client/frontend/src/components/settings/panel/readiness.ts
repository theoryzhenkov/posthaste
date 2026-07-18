import type { UseQueryResult } from '@tanstack/react-query'

import { settingsReadinessStateFromQueries } from '../../../lib/labReadiness'

export function settingsPanelReadiness(input: {
  accountQuery: UseQueryResult<unknown>
  editingSmartMailboxId: string | null
  editorAccountId: string | null
  settingsQuery: UseQueryResult<unknown>
  smartMailboxListQuery: UseQueryResult<unknown>
  smartMailboxQuery: UseQueryResult<unknown>
}) {
  return settingsReadinessStateFromQueries([
    {
      isLoading: input.settingsQuery.isLoading,
      isError: input.settingsQuery.isError,
    },
    {
      isLoading: input.smartMailboxListQuery.isLoading,
      isError: input.smartMailboxListQuery.isError,
    },
    {
      enabled: input.editorAccountId !== null,
      isLoading: input.accountQuery.isLoading,
      isError: input.accountQuery.isError,
    },
    {
      enabled: input.editingSmartMailboxId !== null,
      isLoading: input.smartMailboxQuery.isLoading,
      isError: input.smartMailboxQuery.isError,
    },
  ])
}
