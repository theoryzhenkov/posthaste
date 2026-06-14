import { AlertCircle, Mail } from 'lucide-react'

import { ProgressBar } from '../ui/progress'

export function EmptyMessageDetail() {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 bg-panel px-6">
      <div className="flex size-18 items-center justify-center rounded-2xl border border-border bg-card shadow-[var(--shadow-pane)]">
        <Mail size={30} strokeWidth={1.5} className="text-muted-foreground/55" />
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

export function LoadingMessageDetail({ label }: { label: string }) {
  return (
    <div className="flex h-full flex-col bg-panel">
      <ProgressBar
        label={label}
        className="border-b border-border px-5 py-2"
        compact
      />
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

export function ErrorMessageDetail({ onRetry }: { onRetry: () => void }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 bg-panel">
      <AlertCircle size={32} strokeWidth={1.5} className="text-destructive/50" />
      <p className="text-sm text-destructive">Failed to load conversation</p>
      <button
        type="button"
        className="rounded border border-border px-3 py-1.5 text-xs text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
        onClick={onRetry}
      >
        Try again
      </button>
    </div>
  )
}
