/**
 * Right-pane message detail: metadata header, thread switcher, and email body.
 *
 * Loads both the conversation (for the thread switcher) and the selected
 * message detail (for the body). Messages are deduped by `(sourceId, messageId)`.
 *
 * @spec docs/L1-ui#messagedetail-and-emailframe
 */
import { useEffect, useMemo } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'

import type {
  ConversationView,
  MessageDetail as MessageDetailPayload,
  MessageSummary,
  SourceMessageRef,
} from '../api/types'
import { mailKeys, mergeConversationView } from '../mailState'
import { runtimeViews } from '../runtime/views'
import { useRuntimeObjectView } from '../runtime/useRuntimeObjectView'
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

/**
 * Fold a runtime `messageDetail` snapshot into the message cache. The body is a
 * separate lazy resource (never part of detail), so there is no body to preserve
 * across header updates: a detail payload (attachments present) is complete on
 * its own, and a summary falls through to trigger a detail refetch.
 */
function mergeMessageDetail(
  _previous: MessageDetailPayload | MessageSummary | undefined,
  next: MessageDetailPayload | MessageSummary,
): MessageDetailPayload | MessageSummary {
  return next
}

/** @spec docs/L1-ui#messagedetail-and-emailframe */
interface MessageSelection extends SourceMessageRef {
  conversationId: string
}

/** @spec docs/L1-ui#messagedetail-and-emailframe */
interface MessageDetailProps {
  selection: MessageSelection | null
  onArchive: () => void
  onSnooze: (until: number) => void
  onDiscardDraft?: () => void
  onEditDraft?: () => void
  onForward: () => void
  onOpenFocusedMessage?: () => void
  onReply: () => void
  onReplyAll: () => void
  onSelectMessage: (message: MessageSummary) => void
  onSearch?: (query: string, append?: boolean) => void
  onTag?: () => void
  onToggleFlag?: () => void
  onTrash?: () => void
}

/**
 * Message detail pane with sticky header, thread switcher, and email body.
 *
 * @spec docs/L1-ui#messagedetail-and-emailframe
 */
export function MessageDetail({
  selection,
  onArchive,
  onSnooze,
  onDiscardDraft,
  onEditDraft,
  onForward,
  onOpenFocusedMessage,
  onReply,
  onReplyAll,
  onSelectMessage,
  onSearch,
  onTag,
  onToggleFlag,
  onTrash,
}: MessageDetailProps) {
  const queryClient = useQueryClient()
  const conversationQueryKey = useMemo(
    () =>
      selection
        ? mailKeys.conversation(selection.conversationId)
        : [...mailKeys.conversationRoot, null],
    [selection],
  )
  const messageQueryKey = useMemo(
    () =>
      selection
        ? mailKeys.message(selection.sourceId, selection.messageId)
        : [...mailKeys.messageRoot, null, null],
    [selection],
  )
  const conversationQuery = useQuery({
    queryKey: conversationQueryKey,
    queryFn: () => runtimeViews.mail.conversation(selection!.conversationId),
    enabled: selection !== null,
  })

  const {
    data: messageData,
    error: messageError,
    isFetching: isMessageFetching,
    isLoading: isMessageLoading,
    refetch: refetchMessage,
  } = useQuery({
    queryKey: messageQueryKey,
    queryFn: () =>
      runtimeViews.mail.message(selection!.messageId, selection!.sourceId),
    enabled: selection !== null,
  })

  // 5b-1: layer the runtime's overlay-folded conversation + detail views over
  // the HTTP queries so flag/read/move optimism shows without a cache patch.
  useRuntimeObjectView<ConversationView>({
    enabled: selection !== null,
    family: 'conversation',
    payload: selection ? { conversationId: selection.conversationId } : {},
    queryKey: conversationQueryKey,
    sourceId: selection?.sourceId ?? null,
  })
  useRuntimeObjectView<MessageDetailPayload | MessageSummary>({
    enabled: selection !== null,
    family: 'messageDetail',
    merge: mergeMessageDetail,
    payload: selection
      ? { sourceId: selection.sourceId, messageId: selection.messageId }
      : {},
    queryKey: messageQueryKey,
    sourceId: selection?.sourceId ?? null,
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
        onSnooze={onSnooze}
        onDiscardDraft={onDiscardDraft}
        onEditDraft={onEditDraft}
        onForward={onForward}
        onOpenFocusedMessage={onOpenFocusedMessage}
        onReply={onReply}
        onReplyAll={onReplyAll}
        onSearch={onSearch}
        onTag={onTag}
        onToggleFlag={onToggleFlag}
        onTrash={onTrash}
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
