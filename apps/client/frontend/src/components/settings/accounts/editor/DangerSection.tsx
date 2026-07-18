import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '../../../ui/overlay/alert-dialog'
import { Button } from '../../../ui/form/button'
import { SettingsSection } from '../../panel/shared'
import type {
  AccountActionTarget,
  AccountCommandAction,
} from './AccountActions'

export function DangerSection({
  account,
  onCommand,
  isCommandPending,
}: {
  account: AccountActionTarget
  onCommand: (action: AccountCommandAction, account: AccountActionTarget) => void
  isCommandPending: boolean
}) {
  return (
    <SettingsSection title="Danger" tone="danger" className="pt-16">
      <p className="mb-3 text-[12px] text-muted-foreground">
        Remove this account and its synced local data.
      </p>
      <AlertDialog>
        <AlertDialogTrigger asChild>
          <Button
            size="sm"
            variant="destructive"
            type="button"
            disabled={isCommandPending}
          >
            Delete
          </Button>
        </AlertDialogTrigger>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete account?</AlertDialogTitle>
            <AlertDialogDescription>
              This will permanently remove &ldquo;{account.name}&rdquo; and all
              synced data. This cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => onCommand('delete', account)}
            >
              Delete account
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </SettingsSection>
  )
}
