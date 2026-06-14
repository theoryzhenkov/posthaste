import type { AccountOverview } from '../../../api/types'
import { Button } from '../../ui/button'

export type AccountCommandAction =
  | 'enable'
  | 'disable'
  | 'delete'
  | 'sync'
  | 'repairMetadata'

export function AccountActions({
  account,
  onCommand,
  isCommandPending,
}: {
  account: AccountOverview
  onCommand: (action: AccountCommandAction, account: AccountOverview) => void
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
