import { useCallback } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'

import type { Recipient, ReplyContext, SendMessageInput } from '@/api/types'
import type { ComposeIntent } from '@/composeIntent'
import { invalidateComposeSendReadModels } from '@/domainCache'
import { sendRuntimeMessage } from '@/runtime/adapter'

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
  isForwardUnavailable,
  isWaitingForMessageContext,
  onClose,
  replyContext,
  resolveSubmissionSourceId,
  setErrorMessage,
  setIsReadingAttachments,
}: {
  form: ComposeForm
  intentKind: ComposeIntent['kind']
  isForwardUnavailable: boolean
  isWaitingForMessageContext: boolean
  onClose: () => void
  replyContext: ReplyContext | undefined
  resolveSubmissionSourceId: (from: Recipient | null) => string
  setErrorMessage: (message: string | null) => void
  setIsReadingAttachments: (isReading: boolean) => void
}) {
  const queryClient = useQueryClient()
  const sendMutation = useMutation({
    mutationFn: (variables: { sourceId: string; input: SendMessageInput }) =>
      sendRuntimeMessage(variables),
    onSuccess: async (_result, variables) => {
      await invalidateComposeSendReadModels(queryClient, variables.sourceId)
      toast('Message sent')
      onClose()
    },
    onError: (error) => {
      setErrorMessage(error.message)
    },
  })

  const handleSubmit = useCallback(() => {
    void (async () => {
      if (isForwardUnavailable) {
        setErrorMessage(
          'Forward is disabled until forwarded headers and attachments are implemented.',
        )
        return
      }
      if (isWaitingForMessageContext) {
        setErrorMessage('Wait for the message context to finish loading.')
        return
      }

      const input = buildSendInput(form)
      if (intentKind === 'reply' && replyContext) {
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
    isForwardUnavailable,
    isWaitingForMessageContext,
    replyContext,
    resolveSubmissionSourceId,
    sendMutation,
    setErrorMessage,
    setIsReadingAttachments,
  ])

  return { handleSubmit, isSending: sendMutation.isPending }
}
