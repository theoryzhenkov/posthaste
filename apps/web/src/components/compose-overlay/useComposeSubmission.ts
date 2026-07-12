import { useCallback } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'

import type { Recipient, ReplyContext, SendMessageInput } from '@/api/types'
import { DEFAULT_UNDO_SEND_DELAY_SECONDS } from '@/api/types/settings'
import type { ComposeIntent } from '@/composeIntent'
import { invalidateComposeSendReadModels } from '@/domainCache'
import { queryKeys } from '@/queryKeys'
import { runtimeMutations } from '@/runtime/mutations'
import { runtimeViews } from '@/runtime/views'

import {
  buildSendInput,
  readAttachmentForSend,
  type ComposeForm,
} from '../composeFormHelpers'
import { validateAttachmentLimits } from './attachments'
import { formatScheduledTime } from './sendLaterPresets'
import { validateComposeSubmission } from './validation'

/**
 * Compose submission: immediate send, undo-send, and send-later.
 *
 * ONE mechanism serves undo-send and send-later: the send command's `sendAt`
 * (RFC 3339). The outbox enqueues the send and HOLDS it until due; until then
 * it is cancelable (the Undo toast / the settings Outbox pane discard both
 * route to the same operation-discard, which races the flusher's atomic claim
 * with exactly one winner — a canceled send is never submitted).
 *
 * - Undo-send: `sendAt = now + delay` (the compose settings'
 *   `undoSendDelaySeconds`, default {@link DEFAULT_UNDO_SEND_DELAY_SECONDS}).
 *   The Send button behavior is unchanged; after sending, a countdown toast
 *   offers Undo until the hold expires. Setting the delay to 0 restores the
 *   pre-feature immediate-send path untouched.
 * - Send-later: an explicit schedule from the composer's "Send later" menu.
 *
 * UNDO RESTORES THE FULL COMPOSE: before scheduling, the compose is persisted
 * as a draft (body/recipients/attachments — `onPersistDraft`), and the
 * scheduled send carries `draftId` so the backend consumes that draft only
 * when the send actually fires (D126). Undo cancels the queued send and
 * reopens the composer on the intact draft (`onRestoreDraft`); if the cancel
 * loses the race with the flusher, the user is told the message already went
 * out.
 *
 * OFFLINE SEMANTICS (local-first, surfaced in the toast copy): a scheduled
 * send is NOT a server-side schedule — it fires when Posthaste is next
 * running and online at/after `sendAt`.
 */
export function useComposeSubmission({
  draftKey,
  form,
  intentKind,
  isPreparingMessage,
  onClose,
  onPersistDraft,
  onRestoreDraft,
  onSent,
  replyContext,
  resolveSubmissionSourceId,
  setErrorMessage,
  setIsReadingAttachments,
}: {
  /** The compose session's stable draft key (the undo-restore identity). */
  draftKey: string
  form: ComposeForm
  intentKind: ComposeIntent['kind']
  isPreparingMessage: boolean
  onClose: () => void
  /** Persist the current compose as a draft (full fidelity, attachments included). */
  onPersistDraft?: () => Promise<void>
  /** Reopen the composer on the kept draft after a successful undo. */
  onRestoreDraft?: (sourceId: string, draftKey: string) => void
  onSent?: () => void | Promise<void>
  replyContext: ReplyContext | undefined
  resolveSubmissionSourceId: (from: Recipient | null) => string
  setErrorMessage: (message: string | null) => void
  setIsReadingAttachments: (isReading: boolean) => void
}) {
  const queryClient = useQueryClient()
  const settingsQuery = useQuery({
    queryKey: queryKeys.settings,
    queryFn: runtimeViews.settings.current,
    staleTime: 60_000,
  })
  const undoSendDelaySeconds =
    settingsQuery.data?.compose?.undoSendDelaySeconds ??
    DEFAULT_UNDO_SEND_DELAY_SECONDS

  // The pre-feature immediate path, byte-identical: the optimistic
  // runtime-mutation send (delay 0 / no schedule).
  const sendMutation = useMutation({
    mutationFn: (variables: { sourceId: string; input: SendMessageInput }) =>
      runtimeMutations.messages.send(variables),
    onSuccess: async () => {
      // Discard the autosaved draft now that the message has been sent.
      await onSent?.()
      await invalidateComposeSendReadModels(queryClient)
      toast('Message sent')
      onClose()
    },
    onError: (error) => {
      setErrorMessage(error.message)
    },
  })

  const undoScheduledSend = useCallback(
    async ({
      sourceId,
      operationId,
      toastId,
    }: {
      sourceId: string
      operationId: string
      toastId: string | number
    }) => {
      try {
        await runtimeMutations.messages.discardOperation(sourceId, operationId)
      } catch {
        // The flusher won the race (the op is in flight or already settled):
        // exactly one winner, and it was not the cancel.
        toast.dismiss(toastId)
        toast('Too late to undo — the message was already sent')
        return
      }
      toast.dismiss(toastId)
      toast('Send canceled — your message is back in the composer')
      await queryClient.invalidateQueries({
        queryKey: queryKeys.pendingOperations(sourceId),
      })
      onRestoreDraft?.(sourceId, draftKey)
    },
    [draftKey, onRestoreDraft, queryClient],
  )

  const scheduleMutation = useMutation({
    mutationFn: async (variables: {
      sourceId: string
      input: SendMessageInput
      sendAt: string
      /** Undo-send hold in seconds; null for an explicit send-later schedule. */
      undoWindowSeconds: number | null
    }) => {
      // Persist the compose as a draft FIRST so undo restores full fidelity
      // (attachments included). Best-effort: a failed save never blocks the
      // send — undo then still cancels, it just cannot reopen the content.
      await onPersistDraft?.()
      const response = await runtimeMutations.messages.scheduleSend({
        sourceId: variables.sourceId,
        input: {
          ...variables.input,
          // The originating draft: consumed by the backend only when the
          // send actually fires (D126), so it stays restorable until then.
          draftId: draftKey,
          // For undo-send the DURATION is authoritative (server-stamped
          // deadline, D152); sendAt rides along as display metadata. For an
          // explicit send-later, sendAt is the schedule.
          sendAt: variables.sendAt,
          undoWindowSeconds: variables.undoWindowSeconds,
        },
      })
      return response
    },
    onSuccess: async (response, variables) => {
      await invalidateComposeSendReadModels(queryClient)
      await queryClient.invalidateQueries({
        queryKey: queryKeys.pendingOperations(variables.sourceId),
      })
      onClose()
      const operationId = response.operation?.id
      if (variables.undoWindowSeconds !== null && operationId) {
        showUndoCountdown({
          seconds: variables.undoWindowSeconds,
          onUndo: (toastId) =>
            void undoScheduledSend({
              sourceId: variables.sourceId,
              operationId,
              toastId,
            }),
        })
      } else if (operationId) {
        const toastId = toast(
          `Scheduled for ${formatScheduledTime(response.operation?.sendAt ?? variables.sendAt)} — sends when Posthaste is open`,
          {
            duration: 10_000,
            action: {
              label: 'Undo',
              onClick: () =>
                void undoScheduledSend({
                  sourceId: variables.sourceId,
                  operationId,
                  toastId,
                }),
            },
          },
        )
      }
    },
    onError: (error) => {
      setErrorMessage(error.message)
    },
  })

  /**
   * Shared submit preparation: validation + attachment reads. Returns the
   * ready-to-send input, or null after surfacing the error.
   */
  const prepareInput =
    useCallback(async (): Promise<SendMessageInput | null> => {
      if (isPreparingMessage) {
        setErrorMessage('Wait for the message to finish preparing.')
        return null
      }

      const input = buildSendInput(form)
      if (
        (intentKind === 'reply' || intentKind === 'replyAll') &&
        replyContext
      ) {
        input.inReplyTo = replyContext.inReplyTo
        input.references = replyContext.references
      }
      const validationError = validateComposeSubmission(form, input)
      if (validationError) {
        setErrorMessage(validationError)
        return null
      }
      const attachmentError = validateAttachmentLimits(form.attachments)
      if (attachmentError) {
        setErrorMessage(attachmentError)
        return null
      }
      setErrorMessage(null)
      setIsReadingAttachments(true)
      try {
        input.attachments = await Promise.all(
          form.attachments.map(readAttachmentForSend),
        )
      } catch (error) {
        setErrorMessage(
          error instanceof Error ? error.message : 'Failed to read attachment.',
        )
        return null
      } finally {
        setIsReadingAttachments(false)
      }
      return input
    }, [
      form,
      intentKind,
      isPreparingMessage,
      replyContext,
      setErrorMessage,
      setIsReadingAttachments,
    ])

  const handleSubmit = useCallback(() => {
    void (async () => {
      const input = await prepareInput()
      if (!input) {
        return
      }
      const sourceId = resolveSubmissionSourceId(input.from)
      // The compose key rides EVERY send (D170): the backend materializes
      // what it means at admission — a key naming a known draft makes this a
      // consuming send (the raced-autosave draft can no longer leak); an
      // unknown key is dropped server-side. The client's view is never
      // load-bearing.
      input.draftId = draftKey
      if (undoSendDelaySeconds <= 0) {
        // No hold configured: the pre-feature immediate send path.
        sendMutation.mutate({ sourceId, input })
        return
      }
      scheduleMutation.mutate({
        sourceId,
        input,
        sendAt: new Date(
          Date.now() + undoSendDelaySeconds * 1000,
        ).toISOString(),
        undoWindowSeconds: undoSendDelaySeconds,
      })
    })()
  }, [
    draftKey,
    prepareInput,
    resolveSubmissionSourceId,
    scheduleMutation,
    sendMutation,
    undoSendDelaySeconds,
  ])

  /** Send-later: schedule the send for an explicit future time. */
  const handleSubmitLater = useCallback(
    (sendAt: string) => {
      void (async () => {
        const input = await prepareInput()
        if (!input) {
          return
        }
        scheduleMutation.mutate({
          sourceId: resolveSubmissionSourceId(input.from),
          input,
          sendAt,
          undoWindowSeconds: null,
        })
      })()
    },
    [prepareInput, resolveSubmissionSourceId, scheduleMutation],
  )

  return {
    handleSubmit,
    handleSubmitLater,
    isSending: sendMutation.isPending || scheduleMutation.isPending,
  }
}

/**
 * The undo-send toast: "Sending in Ns — Undo", counting down each second,
 * then flipping to "Message sent" when the hold expires. The countdown is a
 * UI mirror of the backend hold — the authoritative cancel window is the
 * queued (not yet claimed) outbox op, so an undo raced against the flush
 * still has exactly one winner server-side.
 */
function showUndoCountdown({
  seconds,
  onUndo,
}: {
  seconds: number
  onUndo: (toastId: string | number) => void
}) {
  let remaining = seconds
  const render = (id?: string | number): string | number =>
    toast(`Sending in ${remaining}s`, {
      ...(id !== undefined ? { id } : {}),
      duration: (remaining + 1) * 1000,
      action: { label: 'Undo', onClick: () => onUndo(toastId) },
    })
  const toastId = render()
  const timer = setInterval(() => {
    remaining -= 1
    if (remaining <= 0) {
      clearInterval(timer)
      toast.dismiss(toastId)
      toast('Message sent')
      return
    }
    render(toastId)
  }, 1000)
}
