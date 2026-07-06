import { useState } from 'react'
import { toast } from 'sonner'

import { ApiError } from '@/api/errors'
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
import { useMailboxCounts } from '@/live-store/store'

import { useDeleteMailboxMutation } from './useDeleteMailboxMutation'

/**
 * Confirm-with-count dialog for deleting a mailbox. A non-empty mailbox shows
 * "This permanently deletes N messages" (N from the LIVE mailbox count) and
 * requires an explicit confirm before sending `removeEmails=true`; an empty
 * mailbox gets a simpler confirm. If the count changed between load and delete —
 * the backend refuses an empty-looking delete with 409 `mailbox_not_empty` — the
 * dialog re-prompts with the fresh count. Errors surface via toast (never a raw
 * provider string).
 *
 * @spec docs/eph/RFC-L2-mailbox-management
 */
export function DeleteMailboxDialog({
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
  const deleteMailbox = useDeleteMailboxMutation(sourceId)
  // Live COUNTS drive the warning (D116) — fall back to the query's server count
  // before the first frame seeds a live entry.
  const liveCounts = useMailboxCounts(sourceId)[mailbox.id]
  const total = liveCounts ? liveCounts.total : mailbox.totalEmails
  // A 409 backstop: the mailbox looked empty locally but the server rejected the
  // delete because it holds mail. Force the confirm-with-messages path even when
  // the local count still reads 0.
  const [raceDetected, setRaceDetected] = useState(false)
  const isNonEmpty = total > 0 || raceDetected

  const handleOpenChange = (next: boolean) => {
    if (!next) {
      setRaceDetected(false)
    }
    onOpenChange(next)
  }

  const submit = () => {
    if (deleteMailbox.isPending) {
      return
    }
    deleteMailbox.mutate(
      { mailboxId: mailbox.id, removeEmails: isNonEmpty },
      {
        onSuccess: () => handleOpenChange(false),
        onError: (error) => {
          if (error instanceof ApiError && error.code === 'mailbox_not_empty') {
            // The count changed since the dialog opened: re-prompt with the
            // fresh warning and require an explicit second confirm to remove
            // the messages (the next submit sends removeEmails=true).
            setRaceDetected(true)
            toast.warning(
              'This mailbox now has messages. Confirm again to delete them.',
            )
            return
          }
          toast.error("Couldn't delete the mailbox. Please try again.")
        },
      },
    )
  }

  const description = isNonEmpty ? (
    <>
      This permanently deletes{' '}
      <span className="font-semibold">{mailbox.name}</span>
      {total > 0
        ? ` and its ${total} message${total === 1 ? '' : 's'}`
        : ' and all its messages'}
      . This can&apos;t be undone.
    </>
  ) : (
    <>
      Delete the mailbox <span className="font-semibold">{mailbox.name}</span>?
      This can&apos;t be undone.
    </>
  )

  return (
    <AlertDialog open={open} onOpenChange={handleOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Delete mailbox</AlertDialogTitle>
          <AlertDialogDescription>{description}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={deleteMailbox.isPending}>
            Cancel
          </AlertDialogCancel>
          <AlertDialogAction
            disabled={deleteMailbox.isPending}
            onClick={(event) => {
              // Keep the dialog mounted until the mutation resolves; close on
              // success from the mutate callback.
              event.preventDefault()
              submit()
            }}
          >
            {deleteMailbox.isPending
              ? 'Deleting…'
              : isNonEmpty
                ? 'Delete messages'
                : 'Delete'}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
