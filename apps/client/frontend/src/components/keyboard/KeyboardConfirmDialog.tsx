/**
 * Confirm host for keyboard-invoked destructive actions.
 *
 * A registry action carrying `confirm` metadata (delete-permanently) must not
 * run straight from a keystroke. The registry tier parks its runner and this
 * dialog gates it — reusing the app's shared `AlertDialog` pattern so the
 * keyboard path gets the same affordance the context menu / palette does.
 */
import type { ActionConfirm } from '@/commands'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/overlay/alert-dialog'

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
