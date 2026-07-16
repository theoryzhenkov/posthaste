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
} from '@/components/ui/alert-dialog'
import { Input } from '@/components/ui/input'

import { useCreateMailboxMutation } from './useCreateMailboxMutation'

/**
 * A small dialog to create a new top-level mailbox on a source. Reuses the
 * alert-dialog + input primitives. Shows a brief pending state while the
 * synchronous backend create + resync runs; errors are surfaced by the mutation
 * hook's toast (never a raw provider string).
 *
 * @spec docs/eph/RFC-L2-mailbox-management
 */
export function NewMailboxDialog({
  sourceId,
  open,
  onOpenChange,
}: {
  sourceId: string
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const [name, setName] = useState('')
  const createMailbox = useCreateMailboxMutation(sourceId)

  // Reset the field whenever the dialog closes, so a prior attempt's text does
  // not linger on the next open (done in the open-change handler rather than an
  // effect to avoid a cascading render).
  const handleOpenChange = (next: boolean) => {
    if (!next) {
      setName('')
    }
    onOpenChange(next)
  }

  const trimmed = name.trim()
  const canSubmit = trimmed.length > 0 && !createMailbox.isPending

  const submit = () => {
    if (!canSubmit) {
      return
    }
    createMailbox.mutate(trimmed, {
      onSuccess: () => handleOpenChange(false),
    })
  }

  return (
    <AlertDialog open={open} onOpenChange={handleOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>New mailbox</AlertDialogTitle>
          <AlertDialogDescription>
            Create a new mailbox (folder) on this account.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <Input
          autoFocus
          aria-label="Mailbox name"
          placeholder="Mailbox name"
          value={name}
          disabled={createMailbox.isPending}
          onChange={(event) => setName(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault()
              submit()
            }
          }}
        />
        <AlertDialogFooter>
          <AlertDialogCancel disabled={createMailbox.isPending}>
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
            {createMailbox.isPending ? 'Creating…' : 'Create'}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
