import { Loader2, Paperclip, Send } from 'lucide-react'
import type { RefObject } from 'react'

import { cn } from '@/lib/utils'

import { Button } from '../ui/button'

export function ComposeFooter({
  errorMessage,
  fieldsDisabled,
  fileInputRef,
  isReadingAttachments,
  isSending,
  statusLabel,
  onAttachFiles,
  onClose,
  onSubmit,
}: {
  errorMessage: string | null
  fieldsDisabled: boolean
  fileInputRef: RefObject<HTMLInputElement | null>
  isReadingAttachments: boolean
  isSending: boolean
  statusLabel: string
  onAttachFiles: (files: FileList | null) => void
  onClose: () => void
  onSubmit: () => void
}) {
  return (
    <div className="flex min-h-12 shrink-0 items-center gap-3 border-t border-border/70 px-4 py-2">
      <input
        ref={fileInputRef}
        type="file"
        multiple
        className="hidden"
        onChange={(event) => onAttachFiles(event.target.files)}
      />
      <Button
        type="button"
        variant="ghost"
        size="icon"
        className="size-8 shrink-0 text-muted-foreground hover:bg-[var(--hover-bg)]"
        title="Attach files"
        disabled={fieldsDisabled || isSending || isReadingAttachments}
        onClick={() => fileInputRef.current?.click()}
      >
        <Paperclip size={16} />
      </Button>
      <div
        className={cn(
          'min-w-0 flex-1 truncate text-[12px]',
          errorMessage ? 'text-destructive' : 'text-muted-foreground',
        )}
      >
        {errorMessage ??
          (isReadingAttachments ? 'Reading attachments...' : statusLabel)}
      </div>
      <Button
        type="button"
        variant="outline"
        className="border-border bg-background/45 text-foreground hover:bg-[var(--hover-bg)]"
        onClick={onClose}
      >
        Cancel
      </Button>
      <Button
        type="button"
        onClick={onSubmit}
        disabled={isSending || isReadingAttachments || fieldsDisabled}
        className="bg-brand-coral text-white hover:bg-brand-coral/90"
      >
        {isSending || isReadingAttachments ? (
          <Loader2 size={15} className="animate-spin" />
        ) : (
          <Send size={15} />
        )}
        Send
      </Button>
    </div>
  )
}
