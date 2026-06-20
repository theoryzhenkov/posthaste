/**
 * Loads the original message's attachments for a forward compose.
 *
 * Forwarding re-sends the original files, so we fetch the source message's
 * attachment list and download each one through the runtime resource transport,
 * materializing browser `File`s that flow through the normal send pipeline.
 *
 * Inline attachments (embedded images referenced by the HTML body) are skipped:
 * the forwarded body we seed is plain text, so carrying them as separate files
 * would be noise. Regular attachments are preserved.
 *
 * @spec docs/L1-compose#forward-quoting
 */
import { useQuery } from '@tanstack/react-query'

import type { ComposeIntent } from '@/composeIntent'
import { runtimeResources } from '@/runtime/resources'
import { runtimeViews } from '@/runtime/views'

import {
  composeAttachmentFromFile,
  type ComposeAttachment,
} from '../composeFormHelpers'

export interface ForwardAttachmentsResult {
  attachments: ComposeAttachment[]
  isLoading: boolean
  isError: boolean
}

export function useForwardAttachments({
  intent,
}: {
  intent: ComposeIntent
}): ForwardAttachmentsResult {
  const enabled = intent.kind === 'forward'
  const sourceId = enabled ? intent.sourceId : ''
  const messageId = enabled ? intent.messageId : ''

  const query = useQuery({
    queryKey: ['forward-attachments', sourceId, messageId],
    enabled,
    // Original attachments are immutable for a given message; cache aggressively
    // so reopening the same forward does not re-download.
    staleTime: Infinity,
    queryFn: async (): Promise<ComposeAttachment[]> => {
      const detail = await runtimeViews.mail.message(messageId, sourceId)
      const sources = detail.attachments.filter(
        (attachment) => !attachment.isInline,
      )
      return Promise.all(
        sources.map(async (attachment) => {
          const blob = await runtimeResources.blob({
            kind: 'message-attachment',
            sourceId,
            messageId,
            attachmentId: attachment.id,
          })
          const filename = attachment.filename ?? 'attachment'
          const file = new File([blob], filename, {
            type: attachment.mimeType || 'application/octet-stream',
          })
          return composeAttachmentFromFile(file)
        }),
      )
    },
  })

  return {
    attachments: query.data ?? [],
    isLoading: enabled && query.isLoading,
    isError: enabled && query.isError,
  }
}
