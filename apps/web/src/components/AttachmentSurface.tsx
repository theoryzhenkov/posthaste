import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { AlertCircle, Download, FileText } from 'lucide-react'

import { useRuntimeResourceObjectUrl } from '@/hooks/useRuntimeResourceObjectUrl'
import type { MessageAttachment } from '@/api/types'
import { canPreviewAttachment, formatAttachmentSize } from '@/attachments'
import { mailKeys } from '@/mailState'
import { fetchRuntimeMessage } from '@/runtime/adapter'
import type { AttachmentSurfaceDescriptor } from '@/surfaces'
import { Button } from './ui/button'
import { ProgressBar } from './ui/progress'

function AttachmentPreviewContent({
  attachment,
  objectUrl,
}: {
  attachment: MessageAttachment
  objectUrl: string
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
          src={objectUrl}
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
        src={objectUrl}
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
    queryFn: () => fetchRuntimeMessage(messageId, sourceId),
  })
  const attachment =
    messageQuery.data?.attachments.find(
      (candidate) => candidate.id === attachmentId,
    ) ?? null

  // One authenticated fetch of the attachment bytes serves both the preview
  // (<img>/<iframe>) and the download link: the browser can't auth-load either
  // directly, so we hold the blob and point both at its object URL. The
  // `download` attribute supplies the filename, so we don't need the server's
  // `?download=1` content-disposition variant.
  const attachmentResource = attachment
    ? {
        kind: 'message-attachment' as const,
        sourceId,
        messageId,
        attachmentId: attachment.id,
      }
    : null
  const {
    objectUrl,
    isLoading: isBlobLoading,
    error: blobError,
  } = useRuntimeResourceObjectUrl(attachmentResource)

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
        {objectUrl && (
          <Button asChild size="sm" type="button" variant="outline">
            <a download={attachment.filename ?? 'attachment'} href={objectUrl}>
              <Download size={14} strokeWidth={1.75} />
              Download
            </a>
          </Button>
        )}
      </header>

      <div className="min-h-0 flex-1 bg-panel">
        {!canPreviewAttachment(attachment) ? (
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
        ) : blobError ? (
          <div className="flex h-full flex-col items-center justify-center gap-2 bg-panel text-center">
            <AlertCircle
              size={32}
              strokeWidth={1.5}
              className="text-destructive/50"
            />
            <p className="text-sm text-destructive">
              Failed to load attachment preview
            </p>
          </div>
        ) : isBlobLoading || !objectUrl ? (
          <div className="relative flex h-full min-h-0 items-center justify-center bg-panel p-5">
            <ProgressBar
              ariaLabel="Loading attachment preview"
              className="absolute inset-x-5 top-5 z-10"
              compact
            />
          </div>
        ) : (
          <AttachmentPreviewContent
            key={objectUrl}
            attachment={attachment}
            objectUrl={objectUrl}
          />
        )}
      </div>
    </div>
  )
}
