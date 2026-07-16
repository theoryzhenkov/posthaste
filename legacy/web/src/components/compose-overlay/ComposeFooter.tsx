import { useState } from 'react'
import { ChevronDown, Clock3, Loader2, Paperclip, Send } from 'lucide-react'
import type { RefObject } from 'react'

import { cn } from '@/lib/utils'

import { Button } from '../ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '../ui/popover'
import { sendLaterPresets } from './sendLaterPresets'

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
  onSubmitLater,
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
  /** Schedule the send for an explicit RFC 3339 time ("Send later"). */
  onSubmitLater?: (sendAt: string) => void
}) {
  const submitDisabled = isSending || isReadingAttachments || fieldsDisabled
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
      <div className="flex items-center">
        <Button
          type="button"
          onClick={onSubmit}
          disabled={submitDisabled}
          className={cn(
            'bg-brand-coral text-white hover:bg-brand-coral/90',
            onSubmitLater && 'rounded-r-none',
          )}
        >
          {isSending || isReadingAttachments ? (
            <Loader2 size={15} className="animate-spin" />
          ) : (
            <Send size={15} />
          )}
          Send
        </Button>
        {onSubmitLater ? (
          <SendLaterMenu
            disabled={submitDisabled}
            onSubmitLater={onSubmitLater}
          />
        ) : null}
      </div>
    </div>
  )
}

/**
 * The "Send later" half of the Send split-button: schedule presets plus a
 * custom date-time. Local-first semantics are stated in the menu itself —
 * a scheduled send fires when Posthaste is open (and online), not from a
 * server-side schedule.
 */
function SendLaterMenu({
  disabled,
  onSubmitLater,
}: {
  disabled: boolean
  onSubmitLater: (sendAt: string) => void
}) {
  const [open, setOpen] = useState(false)
  const [customValue, setCustomValue] = useState('')
  const schedule = (sendAt: string) => {
    setOpen(false)
    onSubmitLater(sendAt)
  }
  const customDate = customValue ? new Date(customValue) : null
  const customValid = customDate !== null && !Number.isNaN(customDate.getTime())
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          type="button"
          disabled={disabled}
          aria-label="Send later"
          title="Send later"
          className="rounded-l-none border-l border-white/25 bg-brand-coral px-1.5 text-white hover:bg-brand-coral/90"
        >
          <ChevronDown size={14} />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-64 p-2">
        <div className="mb-1 flex items-center gap-1.5 px-1 text-[12px] font-medium text-muted-foreground">
          <Clock3 size={13} />
          Send later
        </div>
        {sendLaterPresets().map((preset) => (
          <Button
            key={preset.label}
            type="button"
            variant="ghost"
            className="w-full justify-between text-[13px]"
            onClick={() => schedule(preset.sendAt)}
          >
            <span>{preset.label}</span>
            <span className="text-muted-foreground">{preset.hint}</span>
          </Button>
        ))}
        <div className="mt-2 border-t border-border/70 pt-2">
          <label className="px-1 text-[12px] text-muted-foreground">
            Custom time
            <input
              type="datetime-local"
              value={customValue}
              onChange={(event) => setCustomValue(event.target.value)}
              className="mt-1 w-full rounded-md border border-border bg-background px-2 py-1 text-[13px] text-foreground"
            />
          </label>
          <Button
            type="button"
            variant="outline"
            className="mt-2 w-full text-[13px]"
            disabled={!customValid}
            onClick={() => {
              if (customValid && customDate) {
                schedule(customDate.toISOString())
              }
            }}
          >
            Schedule send
          </Button>
        </div>
        <p className="mt-2 px-1 text-[11px] leading-snug text-muted-foreground">
          Scheduled mail sends when Posthaste is open and online — it is not
          sent by your mail server on a schedule.
        </p>
      </PopoverContent>
    </Popover>
  )
}
