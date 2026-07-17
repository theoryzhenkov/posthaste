/**
 * Loads the original message's attachments for a forward compose.
 *
 * Forwarding re-sends the original files, so we read the source message's
 * `messageDetail` answer for its attachment list and download each part from
 * the blob endpoint, materializing browser `File`s that flow through the
 * normal send pipeline.
 *
 * Inline attachments (embedded images referenced by the HTML body) are
 * skipped: the forwarded body we seed is plain text, so carrying them as
 * separate files would be noise. Regular attachments are preserved.
 *
 * Deliberately NOT a react-query query: the downloaded bytes are immutable
 * blob content, not a live answer, and the global generation-advance
 * invalidation would re-download them on every mail event while the composer
 * is open. One load per compose anchor, held in local state.
 */
import { useEffect, useState } from 'react'

import type { ComposeIntent } from '@/composeIntent'
import { fetchQuery, useMailClient } from '@/data'
import type { MessageDetailResult } from '@/gen'

import {
  composeAttachmentFromFile,
  type ComposeAttachment,
} from '../composeFormHelpers'

export interface ForwardAttachmentsResult {
  attachments: ComposeAttachment[]
  isLoading: boolean
  isError: boolean
}

interface LoadState {
  key: string | null
  attachments: ComposeAttachment[]
  isLoading: boolean
  isError: boolean
}

const IDLE: LoadState = {
  key: null,
  attachments: [],
  isLoading: false,
  isError: false,
}

export function useForwardAttachments({
  intent,
}: {
  intent: ComposeIntent
}): ForwardAttachmentsResult {
  const client = useMailClient()
  // Both forwarding and resuming a draft re-send the source message's files.
  const enabled = intent.kind === 'forward' || intent.kind === 'draft'
  const sourceId = enabled ? intent.sourceId : ''
  const messageId = enabled ? intent.messageId : ''
  const key = enabled ? `${sourceId}:${messageId}` : null

  const [state, setState] = useState<LoadState>(IDLE)

  useEffect(() => {
    if (!key) {
      setState(IDLE)
      return
    }
    let cancelled = false
    setState({ key, attachments: [], isLoading: true, isError: false })
    void (async () => {
      const detail = await fetchQuery<MessageDetailResult>(client, {
        messageDetail: { accountId: sourceId, messageId },
      })
      const sources = detail.attachments.filter(
        (attachment) => !attachment.isInline,
      )
      return Promise.all(
        sources.map(async (attachment) => {
          const response = await fetch(client.blobUrl(attachment.blobId))
          if (!response.ok) {
            throw new Error(
              `attachment download failed with HTTP ${response.status}`,
            )
          }
          const blob = await response.blob()
          const filename = attachment.filename ?? 'attachment'
          const file = new File([blob], filename, {
            type: attachment.mimeType || 'application/octet-stream',
          })
          return composeAttachmentFromFile(file)
        }),
      )
    })()
      .then((attachments) => {
        if (!cancelled) {
          setState({ key, attachments, isLoading: false, isError: false })
        }
      })
      .catch(() => {
        if (!cancelled) {
          setState({ key, attachments: [], isLoading: false, isError: true })
        }
      })
    return () => {
      cancelled = true
    }
  }, [client, key, sourceId, messageId])

  const current = state.key === key ? state : IDLE
  return {
    attachments: current.attachments,
    isLoading: enabled && (current.isLoading || current.key !== key),
    isError: enabled && current.isError,
  }
}
