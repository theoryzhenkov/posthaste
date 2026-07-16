import type { KeyboardEvent, MouseEvent } from 'react'
import { Download, Ellipsis, Eye, FileText } from 'lucide-react'

import type { MessageAttachment } from '@/api/types'
import { canPreviewAttachment, formatAttachmentSize } from '@/attachments'
import { openFocusedSurface } from '@/hooks/useSurfaceRouting'
import { cn } from '@/lib/utils'
import { downloadRuntimeResource } from '@/lib/downloadRuntimeResource'
import { attachmentSurface } from '@/surfaces'

import { Button } from '../ui/button'

export function MessageAttachments({
  attachments,
  messageId,
  sourceId,
}: {
  attachments: MessageAttachment[]
  messageId: string
  sourceId: string
}) {
  if (attachments.length === 0) {
    return null
  }

  return (
    <div className="shrink-0 space-y-2 border-b border-border/70 px-5 py-2.5">
      <div className="flex items-center justify-between">
        <p className="text-[11px] font-medium uppercase text-muted-foreground">
          Attachments
        </p>
        <p className="font-mono text-[11px] text-muted-foreground">
          {attachments.length} item{attachments.length === 1 ? '' : 's'}
        </p>
      </div>
      <div className="space-y-2">
        {attachments.map((attachment) => (
          <AttachmentRow
            attachment={attachment}
            key={attachment.id}
            messageId={messageId}
            sourceId={sourceId}
          />
        ))}
      </div>
    </div>
  )
}

function AttachmentRow({
  attachment,
  messageId,
  sourceId,
}: {
  attachment: MessageAttachment
  messageId: string
  sourceId: string
}) {
  const canPreview = canPreviewAttachment(attachment)
  const openPreview = () =>
    openFocusedSurface(
      attachmentSurface({ sourceId, messageId, attachmentId: attachment.id }),
    )

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key !== 'Enter' && event.key !== ' ') {
      return
    }
    event.preventDefault()
    openPreview()
  }

  return (
    <div
      className={cn(
        'flex items-center justify-between gap-3 rounded-[6px] border border-border/80 bg-background/30 px-2.5 py-2',
        canPreview &&
          'cursor-pointer transition-colors hover:border-primary/60 hover:bg-background/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 focus-visible:ring-offset-background',
      )}
      aria-label={
        canPreview
          ? `Preview attachment ${attachment.filename ?? 'Unnamed attachment'}`
          : undefined
      }
      onClick={canPreview ? openPreview : undefined}
      onKeyDown={canPreview ? handleKeyDown : undefined}
      role={canPreview ? 'button' : undefined}
      tabIndex={canPreview ? 0 : undefined}
      title={
        canPreview
          ? `Preview ${attachment.filename ?? 'attachment'}`
          : undefined
      }
    >
      <AttachmentMeta attachment={attachment} />
      <AttachmentActions
        attachment={attachment}
        canPreview={canPreview}
        messageId={messageId}
        onPreview={openPreview}
        sourceId={sourceId}
      />
    </div>
  )
}

function AttachmentMeta({ attachment }: { attachment: MessageAttachment }) {
  return (
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
  )
}

function AttachmentActions({
  attachment,
  canPreview,
  messageId,
  onPreview,
  sourceId,
}: {
  attachment: MessageAttachment
  canPreview: boolean
  messageId: string
  onPreview: () => void
  sourceId: string
}) {
  return (
    <div className="flex shrink-0 items-center gap-1">
      {canPreview && (
        <Button
          aria-label={`Preview ${attachment.filename ?? 'attachment'}`}
          onClick={(event: MouseEvent) => {
            event.stopPropagation()
            onPreview()
          }}
          size="icon-sm"
          title="Preview"
          type="button"
          variant="ghost"
        >
          <Eye size={14} strokeWidth={1.75} />
        </Button>
      )}
      <Button
        aria-label={`Download ${attachment.filename ?? 'attachment'}`}
        onClick={(event: MouseEvent) => {
          event.stopPropagation()
          void downloadRuntimeResource(
            {
              kind: 'message-attachment',
              sourceId,
              messageId,
              attachmentId: attachment.id,
            },
            attachment.filename ?? 'attachment',
          ).catch(() => {
            // already logged in the helper; nothing to surface here
          })
        }}
        size="icon-sm"
        title="Download"
        type="button"
        variant="ghost"
      >
        <Download size={14} strokeWidth={1.75} />
      </Button>
      <Button disabled size="icon-sm" type="button" variant="ghost">
        <Ellipsis size={14} strokeWidth={1.75} />
      </Button>
    </div>
  )
}
