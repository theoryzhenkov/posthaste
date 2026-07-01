import { useCallback } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'

import type { Recipient, ReplyContext, SendMessageInput } from '@/api/types'
import type { ComposeIntent } from '@/composeIntent'
import { invalidateComposeSendReadModels } from '@/domainCache'
import { runtimeMutations } from '@/runtime/mutations'

import {
  buildSendInput,
  readAttachmentForSend,
  type ComposeForm,
} from '../composeFormHelpers'
import { validateAttachmentLimits } from './attachments'
import { validateComposeSubmission } from './validation'

export function useComposeSubmission({
  form,
  intentKind,
  isPreparingMessage,
  onClose,
  onSent,
  replyContext,
  resolveSubmissionSourceId,
  setErrorMessage,
  setIsReadingAttachments,
}: {
  form: ComposeForm
  intentKind: ComposeIntent['kind']
  isPreparingMessage: boolean
  onClose: () => void
  onSent?: () => void | Promise<void>
  replyContext: ReplyContext | undefined
  resolveSubmissionSourceId: (from: Recipient | null) => string
  setErrorMessage: (message: string | null) => void
  setIsReadingAttachments: (isReading: boolean) => void
}) {
  const queryClient = useQueryClient()
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

  const handleSubmit = useCallback(() => {
    void (async () => {
      if (isPreparingMessage) {
        setErrorMessage('Wait for the message to finish preparing.')
        return
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
        return
      }
      const attachmentError = validateAttachmentLimits(form.attachments)
      if (attachmentError) {
        setErrorMessage(attachmentError)
        return
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
        return
      } finally {
        setIsReadingAttachments(false)
      }
      sendMutation.mutate({
        sourceId: resolveSubmissionSourceId(input.from),
        input,
      })
    })()
  }, [
    form,
    intentKind,
    isPreparingMessage,
    replyContext,
    resolveSubmissionSourceId,
    sendMutation,
    setErrorMessage,
    setIsReadingAttachments,
  ])

  return { handleSubmit, isSending: sendMutation.isPending }
}
