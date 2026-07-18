import { AlertCircle, X } from 'lucide-react'

export function MessageListErrorBanner({
  errorMessage,
  isInvalidQueryError,
  showClientQueryError,
  onClearSearchQuery,
  onDismiss,
  onRetry,
}: {
  errorMessage: string
  isInvalidQueryError: boolean
  showClientQueryError: boolean
  onClearSearchQuery: () => void
  onDismiss: () => void
  onRetry: () => void
}) {
  return (
    <div className="border-b border-destructive/20 bg-destructive/5 px-3 py-2">
      <div className="flex items-start gap-2 text-sm text-destructive">
        <AlertCircle size={16} strokeWidth={1.8} className="mt-0.5" />
        <p className="min-w-0 flex-1">{errorMessage}</p>
        <button
          type="button"
          className="grid size-6 shrink-0 place-items-center rounded text-destructive/70 transition-colors hover:bg-destructive/10 hover:text-destructive"
          aria-label="Dismiss error"
          onClick={isInvalidQueryError ? onClearSearchQuery : onDismiss}
        >
          <X size={14} />
        </button>
      </div>
      <div className="mt-2 flex gap-2">
        {isInvalidQueryError && (
          <button
            type="button"
            className="rounded border border-destructive/20 px-2 py-1 text-xs text-destructive transition-colors hover:bg-destructive/10"
            onClick={onClearSearchQuery}
          >
            Clear filter
          </button>
        )}
        {!showClientQueryError && (
          <button
            type="button"
            className="rounded border border-destructive/20 px-2 py-1 text-xs text-destructive transition-colors hover:bg-destructive/10"
            onClick={onRetry}
          >
            Try again
          </button>
        )}
      </div>
    </div>
  )
}
