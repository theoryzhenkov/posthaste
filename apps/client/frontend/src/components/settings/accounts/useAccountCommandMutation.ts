import { useMutation } from '@tanstack/react-query'

import { useCommands } from '@/data'
import type { AccountRow } from '@/gen'
import {
  settingsCategorySurface,
  type SettingsSurfaceDescriptor,
} from '@/domain/surface'
import type { AccountActionTarget } from './editor/AccountActions'

/** The lifecycle verbs the accounts pane offers on an existing account.
 * Enable/disable are `updateAccount` patches; delete and sync have their own
 * commands. Every acceptance rides the global invalidation cycle. */
export function useAccountCommandMutation(input: {
  accounts: AccountRow[]
  activeAccountId: string | null
  effectiveEditorTarget: string | null
  onActiveAccountChange: (accountId: string | null) => void
  onNavigate: (surface: SettingsSurfaceDescriptor) => void
  setAccountCommandError: (message: string | null) => void
}) {
  const commands = useCommands()
  return useMutation({
    mutationFn: async ({
      action,
      account,
    }: {
      action: 'enable' | 'disable' | 'delete' | 'sync' | 'repairMetadata'
      account: AccountActionTarget
    }) => {
      switch (action) {
        case 'enable':
          return commands.run({
            updateAccount: { accountId: account.id, enabled: true },
          })
        case 'disable':
          return commands.run({
            updateAccount: { accountId: account.id, enabled: false },
          })
        case 'delete':
          return commands.run({ deleteAccount: { accountId: account.id } })
        case 'sync':
          return commands.run({ syncNow: { accountId: account.id } })
        case 'repairMetadata':
          return commands.run({
            syncNow: { accountId: account.id, mode: 'fullMetadata' },
          })
      }
    },
    onMutate: () => input.setAccountCommandError(null),
    onSuccess: async (_result, variables) => {
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
