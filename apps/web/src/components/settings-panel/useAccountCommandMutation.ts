import { useMutation, type QueryClient } from '@tanstack/react-query'

import type { AccountOverview } from '../../api/types'
import {
  invalidateAccountReadModels,
  removeAccountOverview,
} from '../../domainCache'
import {
  settingsCategorySurface,
  type SettingsSurfaceDescriptor,
} from '../../surfaces'
import {
  deleteRuntimeAccount,
  disableRuntimeAccount,
  enableRuntimeAccount,
} from '../../runtime/accounts'
import { triggerRuntimeSync } from '../../runtime/adapter'

export function useAccountCommandMutation(input: {
  accounts: AccountOverview[]
  activeAccountId: string | null
  effectiveEditorTarget: string | null
  onActiveAccountChange: (accountId: string | null) => void
  onNavigate: (surface: SettingsSurfaceDescriptor) => void
  queryClient: QueryClient
  setAccountCommandError: (message: string | null) => void
}) {
  return useMutation({
    mutationFn: async ({
      action,
      account,
    }: {
      action: 'enable' | 'disable' | 'delete' | 'sync' | 'repairMetadata'
      account: AccountOverview
    }) => {
      switch (action) {
        case 'enable':
          return enableRuntimeAccount(account.id)
        case 'disable':
          return disableRuntimeAccount(account.id)
        case 'delete':
          return deleteRuntimeAccount(account.id)
        case 'sync':
          return triggerRuntimeSync({ sourceId: account.id })
        case 'repairMetadata':
          return triggerRuntimeSync({
            sourceId: account.id,
            mode: 'fullMetadata',
          })
      }
    },
    onMutate: () => input.setAccountCommandError(null),
    onSuccess: async (_result, variables) => {
      if (variables.action === 'delete') {
        removeAccountOverview(input.queryClient, variables.account.id)
        invalidateAccountReadModels(input.queryClient)
      } else {
        invalidateAccountReadModels(input.queryClient, variables.account.id)
      }
      if (variables.action !== 'delete') return
      const fallbackAccountId =
        input.accounts.find(
          (account) =>
            account.id !== variables.account.id &&
            account.enabled &&
            account.isDefault,
        )?.id ??
        input.accounts.find(
          (account) => account.id !== variables.account.id && account.enabled,
        )?.id ??
        null
      if (input.activeAccountId === variables.account.id) {
        input.onActiveAccountChange(fallbackAccountId)
      }
      if (input.effectiveEditorTarget === variables.account.id) {
        input.onNavigate(settingsCategorySurface('accounts'))
      }
    },
    onError: (error: Error) => input.setAccountCommandError(error.message),
  })
}
