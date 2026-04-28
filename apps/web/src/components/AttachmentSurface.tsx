import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { AlertCircle, Download, FileText } from 'lucide-react'

import { buildMessageAttachmentUrl, fetchMessage } from '@/api/client'
import type { MessageAttachment } from '@/api/types'
import { canPreviewAttachment, formatAttachmentSize } from '@/attachments'
import { mailKeys } from '@/mailState'
import type { AttachmentSurfaceDescriptor } from '@/surfaces'
import { Button } from './ui/button'
import { ProgressBar } from './ui/progress'

interface AttachmentPreviewProps {
  attachment: MessageAttachment
  messageId: string
  sourceId: string
}

function AttachmentPreview({
  attachment,
  messageId,
  sourceId,
}: AttachmentPreviewProps) {
  const attachmentUrl = buildMessageAttachmentUrl(
    sourceId,
    messageId,
    attachment.id,
  )

  return (
    <AttachmentPreviewContent
      key={attachmentUrl}
      attachment={attachment}
      attachmentUrl={attachmentUrl}
    />
  )
}

function AttachmentPreviewContent({
  attachment,
  attachmentUrl,
}: {
  attachment: MessageAttachment
  attachmentUrl: string
}) {
  const [isLoadingPreview, setIsLoadingPreview] = useState(true)

  const progress = isLoadingPreview ? (
    <ProgressBar
      ariaLabel="Loading attachment preview"
      className="absolute inset-x-5 top-5 z-10"
      compact
    />
  ) : null

  if (attachment.mimeType.startsWith('image/')) {
    return (
      <div className="relative flex h-full min-h-0 items-center justify-center bg-panel p-5">
        {progress}
        <img
          alt={attachment.filename ?? 'Attachment preview'}
          className="max-h-full max-w-full object-contain"
          onError={() => setIsLoadingPreview(false)}
          onLoad={() => setIsLoadingPreview(false)}
          src={attachmentUrl}
        />
      </div>
    )
  }

  return (
    <div className="relative h-full min-h-0 bg-panel">
      {progress}
      <iframe
        className="h-full w-full border-0 bg-panel"
        onLoad={() => setIsLoadingPreview(false)}
        src={attachmentUrl}
        title={attachment.filename ?? 'Attachment preview'}
      />
    </div>
  )
}

export function AttachmentSurface({
  surface,
}: {
  surface: AttachmentSurfaceDescriptor
}) {
  const { attachmentId, messageId, sourceId } = surface.params
  const messageQuery = useQuery({
    queryKey: mailKeys.message(sourceId, messageId),
    queryFn: () => fetchMessage(messageId, sourceId),
  })
  const attachment =
    messageQuery.data?.attachments.find(
      (candidate) => candidate.id === attachmentId,
    ) ?? null
  const downloadUrl =
    attachment !== null
      ? buildMessageAttachmentUrl(sourceId, messageId, attachment.id, {
          download: true,
        })
      : null

  if (messageQuery.isLoading) {
    return (
      <div className="flex h-full min-h-0 flex-col bg-panel">
        <ProgressBar
          label="Loading attachment"
          className="border-b border-border px-4 py-2"
          compact
        />
        <div className="flex h-[54px] shrink-0 items-center gap-3 border-b border-border px-4">
          <div className="size-8 animate-pulse rounded-[5px] bg-muted" />
          <div className="min-w-0 flex-1 space-y-2">
            <div className="h-3.5 w-52 max-w-full animate-pulse rounded bg-muted" />
            <div className="h-3 w-36 animate-pulse rounded bg-muted/60" />
          </div>
        </div>
        <div className="flex min-h-0 flex-1 items-center justify-center">
          <div className="h-3 w-32 animate-pulse rounded bg-muted/60" />
        </div>
      </div>
    )
  }

  if (messageQuery.error || !messageQuery.data || !attachment) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 bg-panel">
        <AlertCircle
          size={32}
          strokeWidth={1.5}
          className="text-destructive/50"
        />
        <p className="text-sm text-destructive">
          Failed to load attachment preview
        </p>
        <button
          type="button"
          className="rounded border border-border px-3 py-1.5 text-xs text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          onClick={() => void messageQuery.refetch()}
        >
          Try again
        </button>
      </div>
    )
  }

  return (
    <div className="flex h-full min-h-0 flex-col bg-panel">
      <header className="flex h-[54px] shrink-0 items-center gap-3 border-b border-border bg-panel px-4">
        <div className="flex size-8 shrink-0 items-center justify-center rounded-[5px] bg-brand-coral text-brand-coral-foreground">
          <FileText size={16} strokeWidth={1.6} />
        </div>
        <div className="min-w-0 flex-1">
          <p className="truncate text-[13px] font-medium text-foreground">
            {attachment.filename ?? 'Unnamed attachment'}
          </p>
          <p className="mt-0.5 truncate font-mono text-[11px] text-muted-foreground">
            {formatAttachmentSize(attachment.size)}
            <span className="mx-1">·</span>
            {attachment.mimeType}
          </p>
        </div>
        {downloadUrl && (
          <Button asChild size="sm" type="button" variant="outline">
            <a download href={downloadUrl}>
              <Download size={14} strokeWidth={1.75} />
              Download
            </a>
          </Button>
        )}
      </header>

      <div className="min-h-0 flex-1 bg-panel">
        {canPreviewAttachment(attachment) ? (
          <AttachmentPreview
            attachment={attachment}
            messageId={messageId}
            sourceId={sourceId}
          />
        ) : (
          <div className="flex h-full flex-col items-center justify-center gap-2 bg-panel text-center">
            <FileText
              size={34}
              strokeWidth={1.5}
              className="text-muted-foreground/55"
            />
            <p className="text-sm font-medium text-foreground">
              Preview unavailable
            </p>
            <p className="max-w-sm text-xs leading-5 text-muted-foreground">
              This attachment type cannot be previewed in Posthaste yet.
            </p>
          </div>
        )}
      </div>
    </div>
  )
}
