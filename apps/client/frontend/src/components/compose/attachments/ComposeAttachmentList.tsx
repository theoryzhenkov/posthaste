import { Paperclip, X } from 'lucide-react'

import type { ComposeForm } from '../form/model'
import { formatFileSize } from './attachments'

export function ComposeAttachmentList({
  attachments,
  fieldsDisabled,
  isReadingAttachments,
  isSending,
  onRemoveAttachment,
}: {
  attachments: ComposeForm['attachments']
  fieldsDisabled: boolean
  isReadingAttachments: boolean
  isSending: boolean
  onRemoveAttachment: (attachmentId: string) => void
}) {
  if (attachments.length === 0) {
    return null
  }

  return (
    <div className="flex shrink-0 flex-wrap gap-2 border-t border-border/70 px-4 py-2">
      {attachments.map((attachment) => (
        <div
          key={attachment.id}
          className="flex max-w-full items-center gap-2 rounded-full border border-border bg-background/45 px-2.5 py-1 text-[12px] text-muted-foreground"
        >
          <Paperclip size={13} />
          <span className="min-w-0 max-w-56 truncate text-foreground">
            {attachment.filename}
          </span>
          <span>{formatFileSize(attachment.size)}</span>
          <button
            type="button"
            className="rounded-full p-0.5 hover:bg-[var(--hover-bg)]"
            aria-label={`Remove attachment ${attachment.filename}`}
            disabled={fieldsDisabled || isSending || isReadingAttachments}
            onClick={() => onRemoveAttachment(attachment.id)}
          >
            <X size={13} />
          </button>
        </div>
      ))}
    </div>
  )
}
