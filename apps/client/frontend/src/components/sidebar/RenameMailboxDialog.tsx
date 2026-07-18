import { useState } from 'react'

import type { Mailbox } from '@/api/types'
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
import { Input } from '@/components/ui/input'

import { useRenameMailboxMutation } from './useRenameMailboxMutation'

/**
 * A small dialog to rename a mailbox on a source, prefilled with the current
 * name. Reuses the alert-dialog + input primitives (mirroring
 * NewMailboxDialog). Shows a brief pending state while the synchronous
 * backend rename + resync runs; errors are surfaced by the mutation hook's
 * toast (never a raw provider string).
 */
export function RenameMailboxDialog({
  sourceId,
  mailbox,
  open,
  onOpenChange,
}: {
  sourceId: string
  mailbox: Mailbox
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const [name, setName] = useState<string | null>(null)
  const renameMailbox = useRenameMailboxMutation(sourceId)

  // `null` means "untouched" and renders the current name, so every open
  // starts from the live value; the field resets on close in the open-change
  // handler rather than an effect to avoid a cascading render.
  const value = name ?? mailbox.name
  const handleOpenChange = (next: boolean) => {
    if (!next) {
      setName(null)
    }
    onOpenChange(next)
  }

  const trimmed = value.trim()
  const canSubmit =
    trimmed.length > 0 && trimmed !== mailbox.name && !renameMailbox.isPending

  const submit = () => {
    if (!canSubmit) {
      return
    }
    renameMailbox.mutate(
      { mailboxId: mailbox.id, name: trimmed },
      {
        onSuccess: () => handleOpenChange(false),
      },
    )
  }

  return (
    <AlertDialog open={open} onOpenChange={handleOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Rename mailbox</AlertDialogTitle>
          <AlertDialogDescription>
            Rename <span className="font-semibold">{mailbox.name}</span> on
            this account.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <Input
          autoFocus
          aria-label="Mailbox name"
          placeholder="Mailbox name"
          value={value}
          disabled={renameMailbox.isPending}
          onChange={(event) => setName(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault()
              submit()
            }
          }}
        />
        <AlertDialogFooter>
          <AlertDialogCancel disabled={renameMailbox.isPending}>
            Cancel
          </AlertDialogCancel>
          <AlertDialogAction
            disabled={!canSubmit}
            onClick={(event) => {
              // Keep the dialog mounted until the mutation resolves; close on
              // success from the mutate callback.
              event.preventDefault()
              submit()
            }}
          >
            {renameMailbox.isPending ? 'Renaming…' : 'Rename'}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
