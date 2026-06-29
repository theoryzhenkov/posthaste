import { useState, type MouseEvent } from 'react'
import {
  Archive,
  Clock,
  Ellipsis,
  Forward,
  Paperclip,
  Pencil,
  Reply,
  ReplyAll,
} from 'lucide-react'

import { SYSTEM_KEYWORDS } from '@/domainVocabulary'

import type { MessageDetail, MessageSummary } from '@/api/types'

import { Badge } from '../ui/badge'
import { Button } from '../ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '../ui/popover'
import { snoozePresets } from './snoozePresets'
import {
  formatAbsoluteDate,
  formatRecipientEmailList,
  initialsForSender,
  userTags,
} from './model'

export function MessageHeader({
  conversationSubject,
  message,
  onArchive,
  onEditDraft,
  onForward,
  onReply,
  onReplyAll,
  onSearch,
  onSnooze,
  threadMessages,
}: {
  conversationSubject: string | null | undefined
  message: MessageDetail
  onArchive: () => void
  onEditDraft?: () => void
  onForward: () => void
  onReply: () => void
  onReplyAll: () => void
  onSearch?: (query: string, append?: boolean) => void
  onSnooze: (until: number) => void
  threadMessages: MessageSummary[]
}) {
  const isDraft = message.keywords.includes(SYSTEM_KEYWORDS.Draft)
  const senderName = message.fromName ?? message.fromEmail ?? 'Unknown sender'
  const senderEmail = message.fromEmail ?? ''
  const tags = userTags(message.keywords)
  const recipientLabel = `to ${formatRecipientEmailList(message.to)}`

  return (
    <div className="shrink-0 border-b border-border bg-panel px-5 py-4">
      <div className="flex items-start gap-3">
        <div className="flex size-7 shrink-0 items-center justify-center rounded-full bg-brand-coral text-[11px] font-semibold text-brand-coral-foreground">
          {initialsForSender(senderName)}
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div className="min-w-0 space-y-1.5">
              <h2 className="text-[17px] font-semibold leading-tight text-foreground">
                {conversationSubject ?? message.subject ?? '(no subject)'}
              </h2>
              <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-[12px] text-muted-foreground">
                <SenderButtons
                  senderEmail={senderEmail}
                  senderName={senderName}
                  onSearch={onSearch}
                />
                <span className="text-muted-foreground/60">
                  {recipientLabel}
                </span>
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
            <HeaderActions
              isDraft={isDraft}
              onArchive={onArchive}
              onEditDraft={onEditDraft}
              onForward={onForward}
              onReply={onReply}
              onReplyAll={onReplyAll}
              onSnooze={onSnooze}
            />
          </div>
          <MessageTagRow
            tags={tags}
            hasAttachment={message.hasAttachment}
            attachmentCount={message.attachments.length}
            onSearch={onSearch}
          />
        </div>
      </div>
    </div>
  )
}

function SenderButtons({
  senderEmail,
  senderName,
  onSearch,
}: {
  senderEmail: string
  senderName: string
  onSearch?: (query: string, append?: boolean) => void
}) {
  return (
    <span className="inline-flex min-w-0 items-center gap-1.5 text-foreground">
      <button
        className="truncate font-medium hover:text-primary hover:underline"
        onClick={(event) =>
          onSearch?.(`from:${senderEmail || senderName}`, event.shiftKey)
        }
        title="Search emails from this sender"
      >
        {senderName}
      </button>
      {senderEmail && senderName !== senderEmail && (
        <button
          className="font-mono text-[11px] text-muted-foreground hover:text-primary hover:underline"
          onClick={(event) => onSearch?.(`from:${senderEmail}`, event.shiftKey)}
          title="Search emails from this sender"
        >
          &lt;{senderEmail}&gt;
        </button>
      )}
    </span>
  )
}

function HeaderActions({
  isDraft,
  onArchive,
  onEditDraft,
  onForward,
  onReply,
  onReplyAll,
  onSnooze,
}: {
  isDraft: boolean
  onArchive: () => void
  onEditDraft?: () => void
  onForward: () => void
  onReply: () => void
  onReplyAll: () => void
  onSnooze: (until: number) => void
}) {
  const [snoozeOpen, setSnoozeOpen] = useState(false)
  if (isDraft && onEditDraft) {
    return (
      <div className="flex shrink-0 items-center gap-1">
        <Button
          aria-label="Edit draft"
          onClick={onEditDraft}
          size="sm"
          title="Edit draft"
          type="button"
          variant="ghost"
          className="gap-1.5"
        >
          <Pencil size={14} strokeWidth={1.6} />
          Edit draft
        </Button>
      </div>
    )
  }
  return (
    <div className="flex shrink-0 items-center gap-1">
      <Button
        aria-label="Reply"
        onClick={onReply}
        size="icon-sm"
        title="Reply"
        type="button"
        variant="ghost"
      >
        <Reply size={14} strokeWidth={1.6} />
      </Button>
      <Button
        aria-label="Reply All"
        onClick={onReplyAll}
        size="icon-sm"
        title="Reply All"
        type="button"
        variant="ghost"
      >
        <ReplyAll size={14} strokeWidth={1.6} />
      </Button>
      <Button
        aria-label="Forward (not available yet)"
        disabled
        onClick={onForward}
        size="icon-sm"
        title="Forward will be enabled after forwarded headers and attachments are implemented"
        type="button"
        variant="ghost"
      >
        <Forward size={14} strokeWidth={1.6} />
      </Button>
      <Button
        aria-label="Archive"
        onClick={onArchive}
        size="icon-sm"
        title="Archive"
        type="button"
        variant="ghost"
      >
        <Archive size={14} strokeWidth={1.6} />
      </Button>
      <Popover open={snoozeOpen} onOpenChange={setSnoozeOpen}>
        <PopoverTrigger asChild>
          <Button
            aria-label="Snooze"
            size="icon-sm"
            title="Snooze"
            type="button"
            variant="ghost"
          >
            <Clock size={14} strokeWidth={1.6} />
          </Button>
        </PopoverTrigger>
        <PopoverContent align="end" className="w-44">
          {snoozePresets().map((preset) => (
            <Button
              key={preset.label}
              className="w-full justify-start"
              onClick={() => {
                onSnooze(preset.until)
                setSnoozeOpen(false)
              }}
              size="sm"
              type="button"
              variant="ghost"
            >
              {preset.label}
            </Button>
          ))}
        </PopoverContent>
      </Popover>
      <Button disabled size="icon-sm" type="button" variant="ghost">
        <Ellipsis size={14} strokeWidth={1.6} />
      </Button>
    </div>
  )
}

function MessageTagRow({
  attachmentCount,
  hasAttachment,
  tags,
  onSearch,
}: {
  attachmentCount: number
  hasAttachment: boolean
  tags: string[]
  onSearch?: (query: string, append?: boolean) => void
}) {
  if (tags.length === 0 && !hasAttachment && attachmentCount === 0) {
    return null
  }
  return (
    <div className="mt-3 flex flex-wrap items-center gap-2">
      {tags.map((tag) => (
        <Badge
          variant="outline"
          className="cursor-pointer rounded-[4px] border-border/80 bg-background/45 px-1.5 py-0.5 font-mono text-[10px] uppercase text-muted-foreground hover:border-primary hover:text-primary"
          key={tag}
          onClick={(event: MouseEvent) =>
            onSearch?.(`tag:${tag}`, event.shiftKey)
          }
          title={`Search emails tagged "${tag}"`}
        >
          {tag}
        </Badge>
      ))}
      {hasAttachment && (
        <button
          className="inline-flex items-center gap-1.5 rounded-[4px] border border-border/80 bg-background/45 px-2 py-0.5 text-[11px] font-medium text-muted-foreground transition-colors hover:border-primary hover:text-primary"
          onClick={(event) => onSearch?.('has:attachment', event.shiftKey)}
          title="Search emails with attachments"
        >
          <Paperclip size={12} strokeWidth={1.6} />
          Has attachment
        </button>
      )}
    </div>
  )
}
