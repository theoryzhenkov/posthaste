/**
 * Right-pane message detail: metadata header, thread switcher, and email body.
 *
 * Loads the selected message's detail (summary + inline bodies + attachment
 * metadata) and, from its provider thread id, the surrounding thread for the
 * switcher. Messages are deduped by `(sourceId, messageId)`.
 *
 * @spec docs/L1-ui#messagedetail-and-emailframe
 */
import type {
  ConversationView,
  MessageDetail as MessageDetailPayload,
  SourceMessageRef,
} from '../api/types'
import type { MessageSummary } from '@/gen'
import type { EmailActions } from '../hooks/useEmailActions'
import { useMessageDetail, useThread } from '@/data/queries'
import { MessageAttachments } from './message-detail/MessageAttachments'
import { MessageBody } from './message-detail/MessageBody'
import { MessageHeader } from './message-detail/MessageHeader'
import {
  ErrorMessageDetail,
  EmptyMessageDetail,
  LoadingMessageDetail,
} from './message-detail/MessageDetailStates'
import { dedupeConversationMessages } from './message-detail/model'

/** @spec docs/L1-ui#messagedetail-and-emailframe */
interface MessageSelection extends SourceMessageRef {
  conversationId: string
}

/** @spec docs/L1-ui#messagedetail-and-emailframe */
interface MessageDetailProps {
  selection: MessageSelection | null
  /** Domain mutations — the header resolves its action row from the registry
   *  and delegates email operations (archive/trash/flag/snooze/…) here. */
  actions: EmailActions
  /** Role of the current view (null when ambiguous, e.g. the focused message
   *  window) — makes the header's action row role-aware. */
  viewRole: string | null
  onEditDraft?: () => void
  onForward: () => void
  onOpenFocusedMessage?: () => void
  onReply: () => void
  onReplyAll: () => void
  onSelectMessage: (message: MessageSummary) => void
  onSearch?: (query: string, append?: boolean) => void
  onTag?: () => void
  /** Open the composer prefilled from a `mailto:` unsubscribe URI. */
  onUnsubscribeMailto?: (mailtoUri: string) => void
}

/**
 * Message detail pane with sticky header, thread switcher, and email body.
 *
 * @spec docs/L1-ui#messagedetail-and-emailframe
 */
export function MessageDetail({
  selection,
  actions,
  viewRole,
  onEditDraft,
  onForward,
  onOpenFocusedMessage,
  onReply,
  onReplyAll,
  onSelectMessage,
  onSearch,
  onTag,
  onUnsubscribeMailto,
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
    <div className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden bg-panel">
      <MessageHeader
        conversationSubject={conversation.subject}
        message={message}
        actions={actions}
        viewRole={viewRole}
        onEditDraft={onEditDraft}
        onForward={onForward}
        onOpenFocusedMessage={onOpenFocusedMessage}
        onReply={onReply}
        onReplyAll={onReplyAll}
        onUnsubscribeMailto={onUnsubscribeMailto}
        onSearch={onSearch}
        onTag={onTag}
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
