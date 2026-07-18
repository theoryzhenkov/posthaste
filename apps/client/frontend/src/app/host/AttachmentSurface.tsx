import { useState } from 'react'
import { AlertCircle, Download, FileText } from 'lucide-react'

import type { MessageAttachment } from '@/data/transport/api'
import { canPreviewAttachment, formatAttachmentSize } from '@/data/models/attachments'
import { useBlobUrl } from '@/data/transport/blobs'
import { useMessageDetail } from '@/data/queries/queries'
import { downloadFileFromUrl } from '@/lib/download'
import type { AttachmentSurfaceDescriptor } from '@/surfaces'
import { Button } from '../../components/ui/form/button'
import { ProgressBar } from '../../components/ui/display/progress'

function AttachmentPreviewContent({
  attachment,
  url,
}: {
  attachment: MessageAttachment
  url: string
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
          src={url}
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
        src={url}
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
  const blobUrl = useBlobUrl()
  const messageQuery = useMessageDetail({ accountId: sourceId, messageId })
  const attachment =
    messageQuery.data?.attachments.find(
      (candidate) => candidate.id === attachmentId,
    ) ?? null

  // The blob URL is authenticated by its token query parameter, so the
  // preview (<img>/<iframe>) points straight at it; the download button
  // fetches the same URL and hands the bytes to the browser's save
  // interaction under the attachment's filename.
  const attachmentUrl = attachment ? blobUrl(attachment.blobId) : null

  if (messageQuery.isPending) {
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
        {attachmentUrl && (
          <Button
            onClick={() => {
              void downloadFileFromUrl(
                attachmentUrl,
                attachment.filename ?? 'attachment',
              ).catch(() => {
                // already logged in the helper; nothing to surface here
              })
            }}
            size="sm"
            type="button"
            variant="outline"
          >
            <Download size={14} strokeWidth={1.75} />
            Download
          </Button>
        )}
      </header>

      <div className="min-h-0 flex-1 bg-panel">
        {!canPreviewAttachment(attachment) || !attachmentUrl ? (
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
        ) : (
          <AttachmentPreviewContent
            key={attachmentUrl}
            attachment={attachment}
            url={attachmentUrl}
          />
        )}
      </div>
    </div>
  )
}
