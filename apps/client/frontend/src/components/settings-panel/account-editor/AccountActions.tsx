import type { AccountSettingsResult } from '@/gen'
import { Button } from '../../ui/button'

export type AccountCommandAction =
  | 'enable'
  | 'disable'
  | 'delete'
  | 'sync'
  | 'repairMetadata'

/** The target of a header/danger action: id + the fields the fallback and
 * confirm texts render. Satisfied by both `AccountRow` and the settings
 * answer. */
export interface AccountActionTarget {
  id: string
  name: string
  enabled: boolean
}

export function AccountActions({
  account,
  onCommand,
  isCommandPending,
}: {
  account: AccountSettingsResult
  onCommand: (action: AccountCommandAction, account: AccountActionTarget) => void
  isCommandPending: boolean
}) {
  return (
    <div className="flex flex-wrap items-center gap-1">
      <Button
        size="sm"
        variant="ghost"
        type="button"
        onClick={() => onCommand('sync', account)}
        disabled={isCommandPending}
      >
        Sync
      </Button>
      {account.driver === 'jmap' && (
        <Button
          size="sm"
          variant="ghost"
          type="button"
          onClick={() => onCommand('repairMetadata', account)}
          disabled={isCommandPending}
          title="Re-fetch all message metadata for this account"
        >
          Repair metadata
        </Button>
      )}
      <Button
        size="sm"
        variant="ghost"
        type="button"
        onClick={() =>
          onCommand(account.enabled ? 'disable' : 'enable', account)
        }
        disabled={isCommandPending}
      >
        {account.enabled ? 'Disable' : 'Enable'}
      </Button>
    </div>
  )
}
