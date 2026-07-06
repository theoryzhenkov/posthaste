/**
 * Confirm host for keyboard-invoked destructive actions (PLAN-L2, Slice 5).
 *
 * A registry action carrying `confirm` metadata (today: delete-permanently)
 * must NOT run straight from a keystroke. The registry tier parks its runner and
 * this dialog gates it — reusing the app's shared `AlertDialog` pattern (cf.
 * `ComposeCloseConfirmDialog`, account `DangerSection`) so the keyboard path
 * gets the same affordance the context menu / palette would.
 *
 * @spec docs/eph/PLAN-L2-action-registry.md
 */
import type { ActionConfirm } from '@/actions'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'

export function KeyboardConfirmDialog({
  confirm,
  onConfirm,
  onCancel,
}: {
  confirm: ActionConfirm | null
  onConfirm: () => void
  onCancel: () => void
}) {
  return (
    <AlertDialog
      open={confirm !== null}
      onOpenChange={(next) => {
        // Radix requests close on Escape / overlay click / Cancel — treat any
        // dismissal as "do not run the destructive action".
        if (!next) onCancel()
      }}
    >
      <AlertDialogContent size="sm">
        <AlertDialogHeader>
          <AlertDialogTitle>{confirm?.title}</AlertDialogTitle>
          <AlertDialogDescription>
            {confirm?.description}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction onClick={onConfirm}>
            {confirm?.confirmLabel}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
