import { useCallback } from 'react'
import { useMutation } from '@tanstack/react-query'
import { toast } from 'sonner'

import type { Recipient, ReplyContext, SendMessageInput } from '@/data/transport/api'
import { DEFAULT_UNDO_SEND_DELAY_SECONDS } from '@/data/transport/api/settings/settings'
import type { ComposeIntent } from '@/domain/composeIntent'
import { useAppSettings, useCommands } from '@/data'
import { nowMs } from '@/lib/ambient/time'

import {
  buildSendInput,
  readAttachmentForSend,
  toSendMessageRequest,
  type ComposeForm,
} from '../form/model'
import { validateAttachmentLimits } from '../attachments/attachments'
import { formatScheduledTime } from '../shell/sendLaterPresets'
import { validateComposeSubmission } from '../form/validation'

/**
 * Compose submission: immediate send, undo-send, and send-later.
 *
 * ONE mechanism serves undo-send and send-later: the `send` command's hold
 * fields (`sendAt`/`undoWindowSeconds`), which travel inside the request. The
 * backend enqueues the send and HOLDS it until due; until then it is
 * cancelable via the `cancelOperation` command (the Undo toast and the
 * settings Outbox pane both route there, racing the flusher's atomic claim
 * with exactly one winner — a canceled send is never submitted).
 *
 * - Undo-send: the compose settings' `undoSendDelaySeconds` (default
 *   {@link DEFAULT_UNDO_SEND_DELAY_SECONDS}) becomes `undoWindowSeconds`; the
 *   server stamps and judges the deadline on ITS clock, `sendAt` rides along
 *   as display metadata. After sending, a countdown toast offers Undo until
 *   the hold expires. A delay of 0 sends immediately with no hold.
 * - Send-later: an explicit `sendAt` from the composer's "Send later" menu.
 *
 * UNDO RESTORES THE FULL COMPOSE: the held send is one intent — the backend
 * keeps the compose restorable as a draft under the request's `draftId`, and
 * a successful cancel reopens the composer on it via `onRestoreDraft`. If the
 * cancel loses the race with the flusher, the user is told the message
 * already went out.
 *
 * OFFLINE SEMANTICS (surfaced in the toast copy): a scheduled send is NOT a
 * server-side schedule — it fires when Posthaste is next running and online
 * at/after `sendAt`.
 */
export function useComposeSubmission({
  draftKey,
  form,
  intentKind,
  isPreparingMessage,
  onClose,
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
  /** Reopen the composer on the kept draft after a successful undo. */
  onRestoreDraft?: (sourceId: string, draftKey: string) => void
  onSent?: () => void | Promise<void>
  replyContext: ReplyContext | undefined
  resolveSubmissionSourceId: (from: Recipient | null) => string
  setErrorMessage: (message: string | null) => void
  setIsReadingAttachments: (isReading: boolean) => void
}) {
  const commands = useCommands()
  const settingsQuery = useAppSettings()
  const undoSendDelaySeconds =
    settingsQuery.data?.settings.compose?.undoSendDelaySeconds ??
    DEFAULT_UNDO_SEND_DELAY_SECONDS

  // The immediate path: no hold fields on the request, the send flushes as
  // soon as the outbox reaches it.
  const sendMutation = useMutation({
    mutationFn: (variables: { sourceId: string; input: SendMessageInput }) =>
      commands.send(
        variables.sourceId,
        toSendMessageRequest(variables.input, draftKey),
      ),
    onSuccess: async () => {
      // Discard the autosaved draft now that the message has been sent.
      await onSent?.()
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
        await commands.run({
          cancelOperation: { accountId: sourceId, operationId },
        })
      } catch {
        // The flusher won the race (the op is in flight or already settled):
        // exactly one winner, and it was not the cancel.
        toast.dismiss(toastId)
        toast('Too late to undo — the message was already sent')
        return
      }
      toast.dismiss(toastId)
      toast('Send canceled — your message is back in the composer')
      onRestoreDraft?.(sourceId, draftKey)
    },
    [commands, draftKey, onRestoreDraft],
  )

  const scheduleMutation = useMutation({
    mutationFn: (variables: {
      sourceId: string
      input: SendMessageInput
      sendAt: string
      /** Undo-send hold in seconds; null for an explicit send-later schedule. */
      undoWindowSeconds: number | null
    }) =>
      // The held send is ONE intent: the request carries the compose content,
      // its stable draft identity (the undo-restore handle), and the hold.
      // The returned operationId is the cancel handle.
      commands.send(
        variables.sourceId,
        toSendMessageRequest(variables.input, draftKey),
        {
          sendAt: variables.sendAt,
          ...(variables.undoWindowSeconds !== null
            ? { undoWindowSeconds: variables.undoWindowSeconds }
            : {}),
        },
      ),
    onSuccess: (result, variables) => {
      onClose()
      const { operationId } = result
      if (variables.undoWindowSeconds !== null) {
        showUndoCountdown({
          seconds: variables.undoWindowSeconds,
          onUndo: (toastId) =>
            void undoScheduledSend({
              sourceId: variables.sourceId,
              operationId,
              toastId,
            }),
        })
      } else {
        const toastId = toast(
          `Scheduled for ${formatScheduledTime(variables.sendAt)} — sends when Posthaste is open`,
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
      if (undoSendDelaySeconds <= 0) {
        // No hold configured: the immediate send path.
        sendMutation.mutate({ sourceId, input })
        return
      }
      scheduleMutation.mutate({
        sourceId,
        input,
        sendAt: new Date(
          nowMs() + undoSendDelaySeconds * 1000,
        ).toISOString(),
        undoWindowSeconds: undoSendDelaySeconds,
      })
    })()
  }, [
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
