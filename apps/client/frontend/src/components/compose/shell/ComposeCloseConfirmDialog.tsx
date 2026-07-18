import type { ComposeIntent } from '@/domain/composeIntent'
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
import { Button } from '@/components/ui/form/button'

import { composeCloseCopy } from './composeCloseGuard'

/**
 * Close-without-send confirmation. Shown when the user closes a dirty compose
 * (X / Escape / click-away / footer Close). Three actions, matching the app's
 * AlertDialog styling: keep editing (cancel), discard the unsaved content, or
 * save it as a draft.
 */
export function ComposeCloseConfirmDialog({
  open,
  intentKind,
  onKeepEditing,
  onDiscard,
  onSaveAsDraft,
}: {
  open: boolean
  intentKind: ComposeIntent['kind']
  onKeepEditing: () => void
  onDiscard: () => void
  onSaveAsDraft: () => void
}) {
  const copy = composeCloseCopy(intentKind)
  return (
    <AlertDialog
      open={open}
      onOpenChange={(next) => {
        // Radix requests close on Escape / overlay click / Cancel — treat any
        // dismissal as "keep editing" (cancel the close).
        if (!next) {
          onKeepEditing()
        }
      }}
    >
      <AlertDialogContent size="sm">
        <AlertDialogHeader>
          <AlertDialogTitle>{copy.title}</AlertDialogTitle>
          <AlertDialogDescription>{copy.description}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Keep editing</AlertDialogCancel>
          <Button variant="ghost" onClick={onDiscard}>
            Discard
          </Button>
          <AlertDialogAction onClick={onSaveAsDraft}>
            {copy.saveLabel}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
