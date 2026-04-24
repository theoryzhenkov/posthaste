/**
 * Right-pane message detail: metadata header, thread switcher, and email body.
 *
 * Loads both the conversation (for the thread switcher) and the selected
 * message detail (for the body). Messages are deduped by `(sourceId, messageId)`.
 *
 * @spec docs/L1-ui#messagedetail-and-emailframe
 */
import { useEffect, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  AlertCircle,
  Archive,
  Download,
  Ellipsis,
  FileText,
  Forward,
  Mail,
  Paperclip,
  Reply,
} from 'lucide-react'
import {
  buildMessageAttachmentUrl,
  fetchConversation,
  fetchMessage,
} from '../api/client'
import type {
  MessageAttachment,
  MessageSummary,
  SourceMessageRef,
} from '../api/types'
import { cn } from '../lib/utils'
import { mergeConversationView } from '../mailState'
import { Badge } from './ui/badge'
import { Button } from './ui/button'
import { EmailFrame } from './EmailFrame'

/** @spec docs/L1-ui#messagedetail-and-emailframe */
interface MessageSelection extends SourceMessageRef {
  conversationId: string
}

/** @spec docs/L1-ui#messagedetail-and-emailframe */
interface MessageDetailProps {
  selection: MessageSelection | null
  onSelectMessage: (message: MessageSummary) => void
  onSearch?: (query: string, append?: boolean) => void
}

/** Filter out JMAP system keywords (starting with $). */
function userTags(keywords: string[]): string[] {
  return keywords.filter((kw) => !kw.startsWith('$'))
}

/**
 * Deduplicate and sort conversation messages by `(sourceId, messageId)`,
 * ordered by `receivedAt`.
 * @spec docs/L1-ui#messagedetail-and-emailframe
 */
function dedupeConversationMessages(
  messages: MessageSummary[],
): MessageSummary[] {
  const uniqueMessages = new Map<string, MessageSummary>()
  for (const message of messages) {
    uniqueMessages.set(`${message.sourceId}:${message.id}`, message)
  }
  return [...uniqueMessages.values()].sort((left, right) => {
    if (left.receivedAt !== right.receivedAt) {
      return left.receivedAt.localeCompare(right.receivedAt)
    }
    return left.id.localeCompare(right.id)
  })
}

function canPreviewAttachment(attachment: MessageAttachment): boolean {
  return (
    attachment.mimeType.startsWith('image/') ||
    attachment.mimeType === 'application/pdf' ||
    attachment.mimeType.startsWith('text/')
  )
}

function formatAttachmentSize(size: number): string {
  if (size < 1024) {
    return `${size} B`
  }
  if (size < 1024 * 1024) {
    return `${(size / 1024).toFixed(1)} KB`
  }
  return `${(size / (1024 * 1024)).toFixed(1)} MB`
}

function formatAbsoluteDate(value: string): string {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(new Date(value))
}

function AttachmentPreview({
  attachment,
  messageId,
  sourceId,
}: {
  attachment: MessageAttachment
  messageId: string
  sourceId: string
}) {
  const attachmentUrl = buildMessageAttachmentUrl(
    sourceId,
    messageId,
    attachment.id,
  )

  if (attachment.mimeType.startsWith('image/')) {
    return (
      <div className="flex h-full items-center justify-center bg-card">
        <img
          alt={attachment.filename ?? 'Attachment preview'}
          className="max-h-full max-w-full object-contain"
          src={attachmentUrl}
        />
      </div>
    )
  }

  return (
    <iframe
      className="h-full w-full border-0 bg-card"
      src={attachmentUrl}
      title={attachment.filename ?? 'Attachment preview'}
    />
  )
}

/**
 * Message detail pane with sticky header, thread switcher, and email body.
 *
 * @spec docs/L1-ui#messagedetail-and-emailframe
 */
export function MessageDetail({
  selection,
  onSelectMessage,
  onSearch,
}: MessageDetailProps) {
  const queryClient = useQueryClient()
  const selectionKey = selection
    ? `${selection.sourceId}:${selection.messageId}`
    : null
  const [attachmentSelection, setAttachmentSelection] = useState<{
    attachmentId: string | null
    selectionKey: string | null
  }>({ attachmentId: null, selectionKey: null })
  const selectedAttachmentId =
    attachmentSelection.selectionKey === selectionKey
      ? attachmentSelection.attachmentId
      : null
  const conversationQuery = useQuery({
    queryKey: ['conversation', selection?.conversationId],
    queryFn: () => fetchConversation(selection!.conversationId),
    enabled: selection !== null,
  })

  const messageQuery = useQuery({
    queryKey: ['message', selection?.sourceId, selection?.messageId],
    queryFn: () => fetchMessage(selection!.messageId, selection!.sourceId),
    enabled: selection !== null,
  })

  useEffect(() => {
    if (!conversationQuery.data) {
      return
    }
    mergeConversationView(queryClient, conversationQuery.data)
  }, [conversationQuery.data, queryClient])

  if (!selection) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-4 bg-panel px-6">
        <div className="flex size-18 items-center justify-center rounded-2xl border border-border bg-card shadow-[var(--shadow-pane)]">
          <Mail
            size={30}
            strokeWidth={1.5}
            className="text-muted-foreground/55"
          />
        </div>
        <div className="max-w-xs text-center">
          <p className="text-sm font-semibold text-foreground">
            No conversation selected
          </p>
          <p className="mt-1 text-xs leading-5 text-muted-foreground">
            Pick a thread from the list to open the inline reader.
          </p>
        </div>
      </div>
    )
  }

  if (conversationQuery.isLoading || messageQuery.isLoading) {
    return (
      <div className="flex h-full flex-col bg-panel">
        <div className="shrink-0 space-y-3 border-b border-border px-5 py-4">
          <div className="h-5 w-3/4 animate-pulse rounded bg-muted" />
          <div className="flex items-center gap-3">
            <div className="h-3.5 w-32 animate-pulse rounded bg-muted" />
            <div className="h-3 w-20 animate-pulse rounded bg-muted/60" />
          </div>
        </div>
        <div className="flex-1 space-y-3 p-5">
          <div className="h-3 w-full animate-pulse rounded bg-muted/60" />
          <div className="h-3 w-5/6 animate-pulse rounded bg-muted/60" />
          <div className="h-3 w-4/6 animate-pulse rounded bg-muted/40" />
          <div className="h-3 w-3/4 animate-pulse rounded bg-muted/40" />
        </div>
      </div>
    )
  }

  const conversation = conversationQuery.data
  const message = messageQuery.data

  if (
    conversationQuery.error ||
    messageQuery.error ||
    !conversation ||
    !message
  ) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 bg-panel">
        <AlertCircle
          size={32}
          strokeWidth={1.5}
          className="text-destructive/50"
        />
        <p className="text-sm text-destructive">Failed to load conversation</p>
        <button
          type="button"
          className="rounded border border-border px-3 py-1.5 text-xs text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          onClick={() => {
            void conversationQuery.refetch()
            void messageQuery.refetch()
          }}
        >
          Try again
        </button>
      </div>
    )
  }

  const senderName = message.fromName ?? message.fromEmail ?? 'Unknown sender'
  const senderEmail = message.fromEmail ?? ''
  const tags = userTags(message.keywords)
  const threadMessages = dedupeConversationMessages(conversation.messages)
  const selectedAttachment =
    message.attachments.find(
      (attachment) => attachment.id === selectedAttachmentId,
    ) ?? null
  void onSelectMessage

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden bg-panel">
      <div className="shrink-0 border-b border-border bg-panel px-5 py-4">
        <div className="flex items-start gap-3">
          <div className="flex size-7 shrink-0 items-center justify-center rounded-full bg-brand-coral text-[11px] font-semibold text-brand-coral-foreground">
            {senderName
              .split(/\s+/)
              .filter(Boolean)
              .slice(0, 2)
              .map((part) => part[0]?.toUpperCase())
              .join('') || '?'}
          </div>
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="min-w-0 space-y-1.5">
                <h2 className="text-[17px] font-semibold leading-tight text-foreground">
                  {conversation.subject ?? message.subject ?? '(no subject)'}
                </h2>
                <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-[12px] text-muted-foreground">
                  <span className="inline-flex min-w-0 items-center gap-1.5 text-foreground">
                    <button
                      className="truncate font-medium hover:text-primary hover:underline"
                      onClick={(e) =>
                        onSearch?.(
                          `from:${senderEmail || senderName}`,
                          e.shiftKey,
                        )
                      }
                      title="Search emails from this sender"
                    >
                      {senderName}
                    </button>
                    {senderEmail && senderName !== senderEmail && (
                      <button
                        className="font-mono text-[11px] text-muted-foreground hover:text-primary hover:underline"
                        onClick={(e) =>
                          onSearch?.(`from:${senderEmail}`, e.shiftKey)
                        }
                        title="Search emails from this sender"
                      >
                        &lt;{senderEmail}&gt;
                      </button>
                    )}
                  </span>
                  <span className="text-muted-foreground/60">to me</span>
                  <span className="font-mono text-[11px] text-muted-foreground">
                    {formatAbsoluteDate(message.receivedAt)}
                  </span>
                  {threadMessages.length > 1 && (
                    <span className="font-mono text-[11px] text-muted-foreground/80">
                      {threadMessages.length} messages
                    </span>
                  )}
                </div>
              </div>
              <div className="flex shrink-0 items-center gap-1">
                <Button
                  disabled
                  size="icon-sm"
                  title="Reply"
                  type="button"
                  variant="ghost"
                >
                  <Reply size={14} strokeWidth={1.6} />
                </Button>
                <Button
                  disabled
                  size="icon-sm"
                  title="Forward"
                  type="button"
                  variant="ghost"
                >
                  <Forward size={14} strokeWidth={1.6} />
                </Button>
                <Button
                  disabled
                  size="icon-sm"
                  title="Archive"
                  type="button"
                  variant="ghost"
                >
                  <Archive size={14} strokeWidth={1.6} />
                </Button>
                <Button disabled size="icon-sm" type="button" variant="ghost">
                  <Ellipsis size={14} strokeWidth={1.6} />
                </Button>
              </div>
            </div>

            {(tags.length > 0 ||
              message.hasAttachment ||
              message.attachments.length > 0) && (
              <div className="mt-3 flex flex-wrap items-center gap-2">
                {tags.map((tag) => (
                  <Badge
                    variant="outline"
                    className="cursor-pointer rounded-[4px] border-border/80 bg-background/45 px-1.5 py-0.5 font-mono text-[10px] uppercase text-muted-foreground hover:border-primary hover:text-primary"
                    key={tag}
                    onClick={(e: React.MouseEvent) =>
                      onSearch?.(`tag:${tag}`, e.shiftKey)
                    }
                    title={`Search emails tagged "${tag}"`}
                  >
                    {tag}
                  </Badge>
                ))}
                {message.hasAttachment && (
                  <button
                    className="inline-flex items-center gap-1.5 rounded-[4px] border border-border/80 bg-background/45 px-2 py-0.5 text-[11px] font-medium text-muted-foreground transition-colors hover:border-primary hover:text-primary"
                    onClick={(e) => onSearch?.('has:attachment', e.shiftKey)}
                    title="Search emails with attachments"
                  >
                    <Paperclip size={12} strokeWidth={1.6} />
                    Has attachment
                  </button>
                )}
              </div>
            )}
          </div>
        </div>
      </div>

      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
        {message.attachments.length > 0 && (
          <div className="shrink-0 space-y-2 border-b border-border/70 px-5 py-2.5">
            <div className="flex items-center justify-between">
              <p className="text-[11px] font-medium uppercase text-muted-foreground">
                Attachments
              </p>
              <p className="font-mono text-[11px] text-muted-foreground">
                {message.attachments.length} item
                {message.attachments.length === 1 ? '' : 's'}
              </p>
            </div>
            <div className="space-y-2">
              {message.attachments.map((attachment) => {
                const canPreview = canPreviewAttachment(attachment)
                const isSelected = attachment.id === selectedAttachmentId
                const downloadUrl = buildMessageAttachmentUrl(
                  message.sourceId,
                  message.id,
                  attachment.id,
                  { download: true },
                )

                return (
                  <div
                    className={cn(
                      'flex items-center justify-between gap-3 rounded-[6px] border border-border/80 bg-background/30 px-2.5 py-2',
                      isSelected &&
                        'border-primary/60 bg-[color-mix(in_oklab,var(--brand-coral)_14%,transparent)]',
                    )}
                    key={attachment.id}
                  >
                    <div className="flex min-w-0 items-center gap-3">
                      <div className="flex size-8 shrink-0 items-center justify-center rounded-[5px] bg-brand-coral text-brand-coral-foreground">
                        <FileText size={16} strokeWidth={1.6} />
                      </div>
                      <div className="min-w-0">
                        <p className="truncate text-[13px] font-medium text-foreground">
                          {attachment.filename ?? 'Unnamed attachment'}
                        </p>
                        <p className="mt-0.5 font-mono text-[11px] text-muted-foreground">
                          {formatAttachmentSize(attachment.size)}
                          <span className="mx-1">·</span>
                          {attachment.mimeType}
                        </p>
                      </div>
                    </div>
                    <div className="flex shrink-0 items-center gap-1">
                      {canPreview && (
                        <Button
                          onClick={() =>
                            setAttachmentSelection({
                              attachmentId: isSelected ? null : attachment.id,
                              selectionKey,
                            })
                          }
                          size="icon-sm"
                          type="button"
                          variant="ghost"
                        >
                          <Paperclip size={14} strokeWidth={1.75} />
                        </Button>
                      )}
                      <Button
                        asChild
                        size="icon-sm"
                        type="button"
                        variant="ghost"
                      >
                        <a download href={downloadUrl}>
                          <Download size={14} strokeWidth={1.75} />
                        </a>
                      </Button>
                      <Button
                        disabled
                        size="icon-sm"
                        type="button"
                        variant="ghost"
                      >
                        <Ellipsis size={14} strokeWidth={1.75} />
                      </Button>
                    </div>
                  </div>
                )
              })}
            </div>
          </div>
        )}

        {selectedAttachment && (
          <div className="mx-5 mt-4 h-72 shrink-0 overflow-hidden rounded-[6px] border border-border bg-card">
            <AttachmentPreview
              attachment={selectedAttachment}
              messageId={message.id}
              sourceId={message.sourceId}
            />
          </div>
        )}

        <div className="min-h-0 flex-1 overflow-hidden bg-panel">
          {message.bodyText ? (
            <article className="ph-scroll h-full max-w-[720px] overflow-auto px-[22px] py-[18px] text-[13px] leading-[1.6] text-foreground/92">
              {message.bodyText.split(/\n{2,}/).map((paragraph, index) => (
                <p
                  key={`${index}-${paragraph.slice(0, 20)}`}
                  className="mb-4 whitespace-pre-wrap last:mb-0"
                >
                  {paragraph}
                </p>
              ))}
            </article>
          ) : message.bodyHtml ? (
            <div className="ph-scroll h-full max-w-[720px] overflow-auto px-[22px] py-[18px]">
              <EmailFrame
                className="h-full min-h-[480px] bg-card"
                html={message.bodyHtml}
              />
            </div>
          ) : (
            <p className="ph-scroll h-full overflow-auto px-[22px] py-[18px] text-[13px] text-muted-foreground">
              {message.preview ?? 'No content available.'}
            </p>
          )}
        </div>
      </div>
    </div>
  )
}
