import { useState } from 'react'

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
import { Input } from '@/components/ui/form/input'

/**
 * A small dialog to name a sidebar Group — used for both create ("New group")
 * and rename. Groups are purely presentational and synced via settings, so
 * there is no pending/provider state: on submit we fire the optimistic settings
 * mutation and close immediately. Reuses the alert-dialog + input primitives
 * (mirrors NewMailboxDialog).
 *
 */
export function GroupNameDialog({
  mode,
  initialName = '',
  open,
  onOpenChange,
  onSubmit,
}: {
  mode: 'create' | 'rename'
  initialName?: string
  open: boolean
  onOpenChange: (open: boolean) => void
  onSubmit: (name: string) => void
}) {
  const [name, setName] = useState(initialName)

  // Reset the field to the current name whenever the dialog opens, and clear a
  // stale draft when it closes (done in the open-change handler to avoid an
  // effect-driven cascading render, matching NewMailboxDialog).
  const handleOpenChange = (next: boolean) => {
    if (next) {
      setName(initialName)
    }
    onOpenChange(next)
  }

  const trimmed = name.trim()
  const canSubmit = trimmed.length > 0

  const submit = () => {
    if (!canSubmit) {
      return
    }
    onSubmit(trimmed)
    onOpenChange(false)
  }

  return (
    <AlertDialog open={open} onOpenChange={handleOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>
            {mode === 'create' ? 'New group' : 'Rename group'}
          </AlertDialogTitle>
          <AlertDialogDescription>
            {mode === 'create'
              ? 'Create a sidebar group to cluster mailboxes. Groups are visual only and never change your mail.'
              : 'Rename this sidebar group.'}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <Input
          autoFocus
          aria-label="Group name"
          placeholder="Group name"
          value={name}
          onChange={(event) => setName(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault()
              submit()
            }
          }}
        />
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction
            disabled={!canSubmit}
            onClick={(event) => {
              event.preventDefault()
              submit()
            }}
          >
            {mode === 'create' ? 'Create' : 'Rename'}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
