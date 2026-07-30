/**
 * Right-pane message detail: metadata header, thread switcher, and email body.
 *
 * Loads the selected message's detail (summary + inline bodies + attachment
 * metadata) and, from its provider thread id, the surrounding thread for the
 * switcher. Messages are deduped by `(sourceId, messageId)`.
 *
 */
import type {
  ConversationView,
  MessageDetail as MessageDetailPayload,
  SourceMessageRef,
} from '../../../data/transport/api/index'
import type { MessageSummary } from '@/gen'
import type { ResolvedActionView } from '@/lib/command'
import { useMessageDetail, useThread } from '@/data/queries/queries'
import { MessageAttachments } from './MessageAttachments'
import { MessageBody } from './MessageBody'
import { MessageHeader } from './MessageHeader'
import {
  ErrorMessageDetail,
  EmptyMessageDetail,
  LoadingMessageDetail,
} from './MessageDetailStates'
import { dedupeConversationMessages } from './model'

interface MessageSelection extends SourceMessageRef {
  conversationId: string
}

interface MessageDetailProps {
  selection: MessageSelection | null
  /** Host-resolved `detail-header` action row for the loaded message
   *  (`commands/bind.buildDetailHeaderActions`): role-aware and with the
   *  host's callbacks pre-bound, so this pane stays pure UI (R11). */
  headerActionsFor: (message: MessageDetailPayload) => ResolvedActionView[]
  onSelectMessage: (message: MessageSummary) => void
  onSearch?: (query: string, append?: boolean) => void
}

/**
 * Message detail pane with sticky header, thread switcher, and email body.
 */
export function MessageDetail({
  selection,
  headerActionsFor,
  onSelectMessage,
  onSearch,
}: MessageDetailProps) {
  const detailQuery = useMessageDetail(
    {
      accountId: selection?.sourceId ?? '',
      messageId: selection?.messageId ?? '',
    },
    { enabled: selection !== null },
  )
  const detail = detailQuery.data

  // The thread switcher needs the surrounding provider thread; its id rides
  // on the detail's summary, so this follows once the detail lands.
  const threadQuery = useThread(
    {
      accountId: selection?.sourceId ?? '',
      threadId: detail?.summary.sourceThreadId ?? '',
    },
    { enabled: selection !== null && detail !== undefined },
  )

  if (!selection) {
    return <EmptyMessageDetail />
  }

  if (detailQuery.isPending || (detail && threadQuery.isPending)) {
    return <LoadingMessageDetail label="Loading message" />
  }

  const thread = threadQuery.data
  if (detailQuery.error || threadQuery.error || !detail || !thread) {
    return (
      <ErrorMessageDetail
        onRetry={() => {
          void detailQuery.refetch()
          void threadQuery.refetch()
        }}
      />
    )
  }

  // The header renders the legacy flattened detail + conversation shapes,
  // composed here from the two answers.
  const message: MessageDetailPayload = {
    ...detail.summary,
    bodyHtml: detail.bodyHtml,
    bodyText: detail.bodyText,
    attachments: detail.attachments,
    listUnsubscribe: detail.listUnsubscribe ?? null,
  }
  const conversation: ConversationView = {
    id: selection.conversationId,
    subject: detail.summary.subject,
    messages: thread.messages,
  }

  const threadMessages = dedupeConversationMessages(conversation.messages)
  void onSelectMessage

  return (
    <div className="surface-pane flex h-full min-h-0 min-w-0 flex-col overflow-hidden">
      <MessageHeader
        conversationSubject={conversation.subject}
        message={message}
        headerActionsFor={headerActionsFor}
        onSearch={onSearch}
        threadMessages={threadMessages}
      />
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <MessageAttachments
          attachments={message.attachments}
          messageId={message.id}
          sourceId={message.sourceId}
        />
        <div className="min-h-0 flex-1 overflow-hidden">
          <MessageBody message={message} />
        </div>
      </div>
    </div>
  )
}
