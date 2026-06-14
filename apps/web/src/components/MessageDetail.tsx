/**
 * Right-pane message detail: metadata header, thread switcher, and email body.
 *
 * Loads both the conversation (for the thread switcher) and the selected
 * message detail (for the body). Messages are deduped by `(sourceId, messageId)`.
 *
 * @spec docs/L1-ui#messagedetail-and-emailframe
 */
import { useEffect } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'

import { fetchConversation, fetchMessage } from '../api/client'
import type { MessageSummary, SourceMessageRef } from '../api/types'
import { mailKeys, mergeConversationView } from '../mailState'
import { MessageAttachments } from './message-detail/MessageAttachments'
import { MessageBody } from './message-detail/MessageBody'
import { MessageHeader } from './message-detail/MessageHeader'
import {
  ErrorMessageDetail,
  EmptyMessageDetail,
  LoadingMessageDetail,
} from './message-detail/MessageDetailStates'
import {
  dedupeConversationMessages,
  isMessageDetailPayload,
} from './message-detail/model'

/** @spec docs/L1-ui#messagedetail-and-emailframe */
interface MessageSelection extends SourceMessageRef {
  conversationId: string
}

/** @spec docs/L1-ui#messagedetail-and-emailframe */
interface MessageDetailProps {
  selection: MessageSelection | null
  onArchive: () => void
  onForward: () => void
  onReply: () => void
  onSelectMessage: (message: MessageSummary) => void
  onSearch?: (query: string, append?: boolean) => void
}

/**
 * Message detail pane with sticky header, thread switcher, and email body.
 *
 * @spec docs/L1-ui#messagedetail-and-emailframe
 */
export function MessageDetail({
  selection,
  onArchive,
  onForward,
  onReply,
  onSelectMessage,
  onSearch,
}: MessageDetailProps) {
  const queryClient = useQueryClient()
  const conversationQuery = useQuery({
    queryKey: selection
      ? mailKeys.conversation(selection.conversationId)
      : [...mailKeys.conversationRoot, null],
    queryFn: () => fetchConversation(selection!.conversationId),
    enabled: selection !== null,
  })

  const {
    data: messageData,
    error: messageError,
    isFetching: isMessageFetching,
    isLoading: isMessageLoading,
    refetch: refetchMessage,
  } = useQuery({
    queryKey: selection
      ? mailKeys.message(selection.sourceId, selection.messageId)
      : [...mailKeys.messageRoot, null, null],
    queryFn: () => fetchMessage(selection!.messageId, selection!.sourceId),
    enabled: selection !== null,
  })

  useEffect(() => {
    if (!conversationQuery.data) {
      return
    }
    mergeConversationView(queryClient, conversationQuery.data)
  }, [conversationQuery.data, queryClient])

  const conversation = conversationQuery.data
  const hasMessageDetailPayload = isMessageDetailPayload(messageData)
  const message = hasMessageDetailPayload ? messageData : null
  useEffect(() => {
    if (!messageData || hasMessageDetailPayload || isMessageFetching) {
      return
    }
    void refetchMessage()
  }, [hasMessageDetailPayload, isMessageFetching, messageData, refetchMessage])

  if (!selection) {
    return <EmptyMessageDetail />
  }

  if (
    conversationQuery.isLoading ||
    isMessageLoading ||
    (messageData && !hasMessageDetailPayload)
  ) {
    const loadingLabel =
      messageData && !hasMessageDetailPayload
        ? 'Fetching uncached message'
        : 'Loading message'
    return <LoadingMessageDetail label={loadingLabel} />
  }

  if (conversationQuery.error || messageError || !conversation || !message) {
    return (
      <ErrorMessageDetail
        onRetry={() => {
          void conversationQuery.refetch()
          void refetchMessage()
        }}
      />
    )
  }

  const threadMessages = dedupeConversationMessages(conversation.messages)
  void onSelectMessage

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden bg-panel">
      <MessageHeader
        conversationSubject={conversation.subject}
        message={message}
        onArchive={onArchive}
        onForward={onForward}
        onReply={onReply}
        onSearch={onSearch}
        threadMessages={threadMessages}
      />
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <MessageAttachments
          attachments={message.attachments}
          messageId={message.id}
          sourceId={message.sourceId}
        />
        <div className="min-h-0 flex-1 overflow-hidden bg-panel">
          <MessageBody message={message} />
        </div>
      </div>
    </div>
  )
}
